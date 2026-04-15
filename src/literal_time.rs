use crate::diagnostics::Diagnostic;
use crate::time::Timestamp;
use std::ffi::OsStr;
use std::str;

const NANOS_PER_SECOND: i32 = 1_000_000_000;

pub fn parse_literal_time(raw: &OsStr) -> Result<Timestamp, Diagnostic> {
    let rendered = str::from_utf8(raw.as_encoded_bytes())
        .map_err(|_| unsupported_literal_time(raw.to_string_lossy().as_ref()))?;

    if let Some(rest) = rendered.strip_prefix('@') {
        return parse_epoch_seconds(rest, rendered);
    }

    if rendered.len() == 10 && rendered.as_bytes()[4] == b'-' && rendered.as_bytes()[7] == b'-' {
        return parse_date_only(rendered);
    }

    parse_date_time(rendered)
}

fn parse_epoch_seconds(raw: &str, original: &str) -> Result<Timestamp, Diagnostic> {
    let (negative, body) = match raw.as_bytes().first() {
        Some(b'+') => (false, &raw[1..]),
        Some(b'-') => (true, &raw[1..]),
        _ => (false, raw),
    };

    let (whole_part, frac_part) = match body.split_once('.') {
        Some((whole, frac)) if !frac.is_empty() => (whole, Some(frac)),
        Some(_) => return Err(unsupported_literal_time(original)),
        None => (body, None),
    };

    if whole_part.is_empty() || !whole_part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(unsupported_literal_time(original));
    }

    let whole = whole_part
        .parse::<i64>()
        .map_err(|_| unsupported_literal_time(original))?;
    let nanos = frac_part
        .map(|frac| parse_fractional_nanos(frac, original))
        .transpose()?
        .unwrap_or(0);

    if !negative {
        return Ok(Timestamp::new(whole, nanos));
    }

    if nanos == 0 {
        Ok(Timestamp::new(-whole, 0))
    } else {
        Ok(Timestamp::new(-whole - 1, NANOS_PER_SECOND - nanos))
    }
}

fn parse_date_only(raw: &str) -> Result<Timestamp, Diagnostic> {
    let (year, month, day) = parse_ymd(raw)?;
    local_timestamp(year, month, day, 0, 0, 0, 0, raw)
}

fn parse_date_time(raw: &str) -> Result<Timestamp, Diagnostic> {
    let (date_part, time_part) = raw
        .split_once('T')
        .or_else(|| raw.split_once(' '))
        .ok_or_else(|| unsupported_literal_time(raw))?;
    let (year, month, day) = parse_ymd(date_part)?;
    let (hour, minute, second, nanos, offset_seconds) = parse_clock(time_part, raw)?;

    match offset_seconds {
        Some(_) => fixed_offset_timestamp(year, month, day, hour, minute, second, nanos),
        None => local_timestamp(year, month, day, hour, minute, second, nanos, raw),
    }
    .map(|timestamp| {
        if let Some(offset) = offset_seconds {
            Timestamp::new(timestamp.seconds - offset as i64, timestamp.nanos)
        } else {
            timestamp
        }
    })
}

fn parse_ymd(raw: &str) -> Result<(i32, u32, u32), Diagnostic> {
    if raw.len() != 10 || raw.as_bytes()[4] != b'-' || raw.as_bytes()[7] != b'-' {
        return Err(unsupported_literal_time(raw));
    }

    let year = parse_digits::<i32>(&raw[0..4], raw)?;
    let month = parse_digits::<u32>(&raw[5..7], raw)?;
    let day = parse_digits::<u32>(&raw[8..10], raw)?;
    ensure_valid_date(year, month, day, raw)?;
    Ok((year, month, day))
}

fn parse_clock(raw: &str, original: &str) -> Result<(u32, u32, u32, i32, Option<i32>), Diagnostic> {
    let (clock_part, offset_seconds) = if let Some(clock) = raw.strip_suffix('Z') {
        (clock, Some(0))
    } else if let Some(index) = find_offset_start(raw) {
        let (clock, offset) = raw.split_at(index);
        (clock, Some(parse_offset(offset, original)?))
    } else {
        (raw, None)
    };

    let (base, frac) = match clock_part.split_once('.') {
        Some((base, frac)) if !frac.is_empty() => (base, Some(frac)),
        Some(_) => return Err(unsupported_literal_time(original)),
        None => (clock_part, None),
    };

    let mut fields = base.split(':');
    let hour = fields
        .next()
        .ok_or_else(|| unsupported_literal_time(original))?;
    let minute = fields
        .next()
        .ok_or_else(|| unsupported_literal_time(original))?;
    let second = fields.next();
    if fields.next().is_some() {
        return Err(unsupported_literal_time(original));
    }

    if hour.len() != 2 || minute.len() != 2 || second.is_some_and(|value| value.len() != 2) {
        return Err(unsupported_literal_time(original));
    }

    let hour = parse_digits::<u32>(hour, original)?;
    let minute = parse_digits::<u32>(minute, original)?;
    let second = second
        .map(|value| parse_digits::<u32>(value, original))
        .transpose()?
        .unwrap_or(0);
    let nanos = frac
        .map(|value| parse_fractional_nanos(value, original))
        .transpose()?
        .unwrap_or(0);

    ensure_valid_time(hour, minute, second, original)?;
    Ok((hour, minute, second, nanos, offset_seconds))
}

fn find_offset_start(raw: &str) -> Option<usize> {
    raw.char_indices()
        .skip(1)
        .find_map(|(index, ch)| matches!(ch, '+' | '-').then_some(index))
}

fn parse_offset(raw: &str, original: &str) -> Result<i32, Diagnostic> {
    let sign = match raw.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(unsupported_literal_time(original)),
    };

    let body = &raw[1..];
    let (hours, minutes) = match body.len() {
        2 => (body, "00"),
        5 if body.as_bytes()[2] == b':' => (&body[..2], &body[3..]),
        _ => return Err(unsupported_literal_time(original)),
    };

    let hours = parse_digits::<i32>(hours, original)?;
    let minutes = parse_digits::<i32>(minutes, original)?;
    if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
        return Err(unsupported_literal_time(original));
    }

    Ok(sign * ((hours * 3600) + (minutes * 60)))
}

fn parse_fractional_nanos(raw: &str, original: &str) -> Result<i32, Diagnostic> {
    if !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(unsupported_literal_time(original));
    }

    let mut digits = raw.as_bytes()[..raw.len().min(9)].to_vec();
    while digits.len() < 9 {
        digits.push(b'0');
    }

    let rendered = str::from_utf8(&digits).expect("fraction digits are ascii");
    rendered
        .parse::<i32>()
        .map_err(|_| unsupported_literal_time(original))
}

fn local_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: i32,
    original: &str,
) -> Result<Timestamp, Diagnostic> {
    ensure_valid_date(year, month, day, original)?;
    ensure_valid_time(hour, minute, second, original)?;

    let mut tm = libc::tm {
        tm_sec: second as i32,
        tm_min: minute as i32,
        tm_hour: hour as i32,
        tm_mday: day as i32,
        tm_mon: month as i32 - 1,
        tm_year: year - 1900,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: -1,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };

    let seconds = unsafe { libc::mktime(&mut tm) };
    if tm.tm_sec != second as i32
        || tm.tm_min != minute as i32
        || tm.tm_hour != hour as i32
        || tm.tm_mday != day as i32
        || tm.tm_mon != month as i32 - 1
        || tm.tm_year != year - 1900
    {
        return Err(unsupported_literal_time(original));
    }

    Ok(Timestamp::new(seconds as i64, nanos))
}

fn fixed_offset_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: i32,
) -> Result<Timestamp, Diagnostic> {
    ensure_valid_date(year, month, day, "fixed-offset timestamp")?;
    ensure_valid_time(hour, minute, second, "fixed-offset timestamp")?;

    let days = days_from_civil(year, month, day);
    let day_seconds = (hour as i64 * 3600) + (minute as i64 * 60) + second as i64;
    Ok(Timestamp::new(days * 86_400 + day_seconds, nanos))
}

fn parse_digits<T>(raw: &str, original: &str) -> Result<T, Diagnostic>
where
    T: str::FromStr,
{
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(unsupported_literal_time(original));
    }

    raw.parse::<T>()
        .map_err(|_| unsupported_literal_time(original))
}

fn ensure_valid_date(year: i32, month: u32, day: u32, original: &str) -> Result<(), Diagnostic> {
    if !(1..=12).contains(&month) {
        return Err(unsupported_literal_time(original));
    }

    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        return Err(unsupported_literal_time(original));
    }

    Ok(())
}

fn ensure_valid_time(
    hour: u32,
    minute: u32,
    second: u32,
    original: &str,
) -> Result<(), Diagnostic> {
    if hour > 23 || minute > 59 || second > 59 {
        return Err(unsupported_literal_time(original));
    }

    Ok(())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = ((153 * shifted_month) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era - 719_468) as i64
}

fn unsupported_literal_time(raw: &str) -> Diagnostic {
    Diagnostic::new(
        format!("unsupported literal time format for `-newerXY`: `{raw}`"),
        1,
    )
}
