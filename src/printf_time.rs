#![cfg_attr(windows, allow(dead_code))]

use crate::diagnostics::Diagnostic;
use crate::printf::PrintfTimeSelector;
use crate::time::Timestamp;
use std::mem::MaybeUninit;

const WEEKDAYS_ABBR: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
const WEEKDAYS_FULL: [&[u8]; 7] = [
    b"Sunday",
    b"Monday",
    b"Tuesday",
    b"Wednesday",
    b"Thursday",
    b"Friday",
    b"Saturday",
];
const MONTHS_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];
const MONTHS_FULL: [&[u8]; 12] = [
    b"January",
    b"February",
    b"March",
    b"April",
    b"May",
    b"June",
    b"July",
    b"August",
    b"September",
    b"October",
    b"November",
    b"December",
];

#[derive(Debug, Clone)]
pub(crate) struct ResolvedTimeParts {
    pub timestamp: Timestamp,
    pub local: libc::tm,
    pub timezone_name: Vec<u8>,
    pub utc_offset_seconds: i32,
}

impl ResolvedTimeParts {
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn for_tests(
        timestamp: Timestamp,
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        weekday: i32,
        yearday: i32,
        is_dst: i32,
        utc_offset_seconds: i32,
        timezone_name: Vec<u8>,
    ) -> Self {
        Self {
            timestamp,
            local: tm_from_parts(
                year,
                month,
                day,
                hour,
                minute,
                second,
                weekday,
                yearday,
                is_dst,
                utc_offset_seconds,
            ),
            timezone_name,
            utc_offset_seconds,
        }
    }
}

pub(crate) fn resolve_local_time_parts(
    timestamp: Timestamp,
) -> Result<ResolvedTimeParts, Diagnostic> {
    let local = local_time(timestamp.seconds)?;
    Ok(ResolvedTimeParts {
        timestamp,
        timezone_name: timezone_name_bytes(&local),
        utc_offset_seconds: utc_offset_seconds(&local),
        local,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
fn tm_from_parts(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    weekday: i32,
    yearday: i32,
    is_dst: i32,
    _utc_offset_seconds: i32,
) -> libc::tm {
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    local.tm_sec = second;
    local.tm_min = minute;
    local.tm_hour = hour;
    local.tm_mday = day;
    local.tm_mon = month - 1;
    local.tm_year = year - 1900;
    local.tm_wday = weekday;
    local.tm_yday = yearday - 1;
    local.tm_isdst = is_dst;
    let _ = _utc_offset_seconds;
    local
}

fn local_time(seconds: i64) -> Result<libc::tm, Diagnostic> {
    let raw = seconds as libc::time_t;

    #[cfg(unix)]
    {
        let mut local = MaybeUninit::<libc::tm>::uninit();
        let ptr = unsafe { libc::localtime_r(&raw, local.as_mut_ptr()) };
        if ptr.is_null() {
            return Err(Diagnostic::new(
                "failed to resolve local time for -printf",
                1,
            ));
        }
        Ok(unsafe { local.assume_init() })
    }

    #[cfg(windows)]
    {
        let mut local = MaybeUninit::<libc::tm>::zeroed();
        let status = unsafe { libc::localtime_s(local.as_mut_ptr(), &raw) };
        if status != 0 {
            return Err(Diagnostic::new(
                "failed to resolve local time for -printf",
                1,
            ));
        }
        Ok(unsafe { local.assume_init() })
    }
}

#[cfg(unix)]
fn utc_offset_seconds(local: &libc::tm) -> i32 {
    strftime_bytes(local, b"%z\0")
        .and_then(|bytes| parse_numeric_utc_offset(&bytes))
        .unwrap_or(0)
}

#[cfg(windows)]
fn utc_offset_seconds(_local: &libc::tm) -> i32 {
    0
}

pub(crate) fn render_full_time_bytes(parts: &ResolvedTimeParts) -> Result<Vec<u8>, Diagnostic> {
    Ok(format!(
        "{} {} {:>2} {:02}:{:02}:{} {}",
        std::str::from_utf8(WEEKDAYS_ABBR[parts.local.tm_wday as usize]).unwrap(),
        std::str::from_utf8(MONTHS_ABBR[parts.local.tm_mon as usize]).unwrap(),
        parts.local.tm_mday,
        parts.local.tm_hour,
        parts.local.tm_min,
        seconds_with_fraction(parts.local.tm_sec, parts.timestamp.nanos),
        parts.local.tm_year + 1900,
    )
    .into_bytes())
}

pub(crate) fn render_selector_bytes(
    parts: &ResolvedTimeParts,
    selector: PrintfTimeSelector,
) -> Result<Vec<u8>, Diagnostic> {
    match selector {
        PrintfTimeSelector::EpochSeconds => Ok(render_epoch_seconds(parts.timestamp)),
        PrintfTimeSelector::GnuPlus => Ok(render_gnu_plus(parts)),
        PrintfTimeSelector::Byte(byte) => match byte {
            b'a' => Ok(WEEKDAYS_ABBR[parts.local.tm_wday as usize].to_vec()),
            b'A' => Ok(WEEKDAYS_FULL[parts.local.tm_wday as usize].to_vec()),
            b'b' | b'h' => Ok(MONTHS_ABBR[parts.local.tm_mon as usize].to_vec()),
            b'B' => Ok(MONTHS_FULL[parts.local.tm_mon as usize].to_vec()),
            b'c' => Ok(render_c_locale_datetime(parts)),
            b'd' => Ok(format!("{:02}", parts.local.tm_mday).into_bytes()),
            b'D' | b'x' => Ok(render_month_day_year(parts)),
            b'F' => Ok(render_iso_date(parts)),
            b'g' | b'G' | b'V' => render_iso_week_fields(parts, byte),
            b'H' => Ok(format!("{:02}", parts.local.tm_hour).into_bytes()),
            b'I' => Ok(format!("{:02}", hour_12(parts.local.tm_hour)).into_bytes()),
            b'j' => Ok(format!("{:03}", parts.local.tm_yday + 1).into_bytes()),
            b'M' => Ok(format!("{:02}", parts.local.tm_min).into_bytes()),
            b'm' => Ok(format!("{:02}", parts.local.tm_mon + 1).into_bytes()),
            b'p' => Ok(if parts.local.tm_hour < 12 {
                b"AM".to_vec()
            } else {
                b"PM".to_vec()
            }),
            b'r' => Ok(format!(
                "{:02}:{:02}:{} {}",
                hour_12(parts.local.tm_hour),
                parts.local.tm_min,
                seconds_without_fraction(parts.local.tm_sec),
                am_pm(parts.local.tm_hour)
            )
            .into_bytes()),
            b'R' => Ok(format!("{:02}:{:02}", parts.local.tm_hour, parts.local.tm_min).into()),
            b'S' => Ok(seconds_with_fraction(parts.local.tm_sec, parts.timestamp.nanos).into()),
            b't' => Ok(vec![b'\t']),
            b'T' | b'X' => Ok(format!(
                "{:02}:{:02}:{}",
                parts.local.tm_hour,
                parts.local.tm_min,
                seconds_with_fraction(parts.local.tm_sec, parts.timestamp.nanos)
            )
            .into_bytes()),
            b'u' => Ok(weekday_monday_one(parts.local.tm_wday)
                .to_string()
                .into_bytes()),
            b'U' | b'W' => Ok(render_week_number(parts, byte).into_bytes()),
            b'w' => Ok(parts.local.tm_wday.to_string().into_bytes()),
            b'Y' => Ok(format!("{:04}", parts.local.tm_year + 1900).into_bytes()),
            b'y' => Ok(format!("{:02}", (parts.local.tm_year + 1900) % 100).into_bytes()),
            b'Z' => Ok(parts.timezone_name.clone()),
            b'z' => Ok(render_numeric_offset(parts.utc_offset_seconds)),
            other => Err(Diagnostic::new(
                format!(
                    "internal error: time selector {} not implemented yet",
                    char::from(other)
                ),
                1,
            )),
        },
    }
}

fn render_epoch_seconds(timestamp: Timestamp) -> Vec<u8> {
    format!("{}.{:09}0", timestamp.seconds, timestamp.nanos).into_bytes()
}

fn render_gnu_plus(parts: &ResolvedTimeParts) -> Vec<u8> {
    format!(
        "{:04}-{:02}-{:02}+{:02}:{:02}:{}",
        parts.local.tm_year + 1900,
        parts.local.tm_mon + 1,
        parts.local.tm_mday,
        parts.local.tm_hour,
        parts.local.tm_min,
        seconds_with_fraction(parts.local.tm_sec, parts.timestamp.nanos),
    )
    .into_bytes()
}

fn seconds_with_fraction(second: i32, nanos: i32) -> String {
    format!("{second:02}.{nanos:09}0")
}

fn render_numeric_offset(offset_seconds: i32) -> Vec<u8> {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute = offset_seconds.abs();
    let hours = absolute / 3600;
    let minutes = (absolute % 3600) / 60;
    format!("{sign}{hours:02}{minutes:02}").into_bytes()
}

#[cfg(unix)]
fn strftime_bytes(local: &libc::tm, format: &[u8]) -> Option<Vec<u8>> {
    debug_assert_eq!(format.last(), Some(&0));

    let mut capacity = 32usize;
    while capacity <= 1024 {
        let mut buffer = vec![0u8; capacity];
        let written = unsafe {
            libc::strftime(
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                format.as_ptr().cast(),
                local,
            )
        };
        if written != 0 {
            buffer.truncate(written);
            return Some(buffer);
        }
        capacity *= 2;
    }

    None
}

#[cfg(unix)]
fn parse_numeric_utc_offset(bytes: &[u8]) -> Option<i32> {
    let (sign, digits) = match bytes.split_first()? {
        (b'+', digits) => (1, digits),
        (b'-', digits) => (-1, digits),
        _ => return None,
    };

    let (hours, minutes) = match digits {
        [h1, h2, m1, m2] => ([*h1, *h2], [*m1, *m2]),
        [h1, h2, b':', m1, m2] => ([*h1, *h2], [*m1, *m2]),
        _ => return None,
    };

    let hours = parse_two_ascii_digits(hours)?;
    let minutes = parse_two_ascii_digits(minutes)?;
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(unix)]
fn parse_two_ascii_digits(bytes: [u8; 2]) -> Option<i32> {
    if !bytes.into_iter().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    Some(((bytes[0] - b'0') as i32 * 10) + (bytes[1] - b'0') as i32)
}

fn render_c_locale_datetime(parts: &ResolvedTimeParts) -> Vec<u8> {
    format!(
        "{} {} {:>2} {:02}:{:02}:{} {}",
        std::str::from_utf8(WEEKDAYS_ABBR[parts.local.tm_wday as usize]).unwrap(),
        std::str::from_utf8(MONTHS_ABBR[parts.local.tm_mon as usize]).unwrap(),
        parts.local.tm_mday,
        parts.local.tm_hour,
        parts.local.tm_min,
        seconds_without_fraction(parts.local.tm_sec),
        parts.local.tm_year + 1900,
    )
    .into_bytes()
}

fn render_month_day_year(parts: &ResolvedTimeParts) -> Vec<u8> {
    format!(
        "{:02}/{:02}/{:02}",
        parts.local.tm_mon + 1,
        parts.local.tm_mday,
        (parts.local.tm_year + 1900) % 100
    )
    .into_bytes()
}

fn render_iso_date(parts: &ResolvedTimeParts) -> Vec<u8> {
    format!(
        "{:04}-{:02}-{:02}",
        parts.local.tm_year + 1900,
        parts.local.tm_mon + 1,
        parts.local.tm_mday
    )
    .into_bytes()
}

fn render_iso_week_fields(parts: &ResolvedTimeParts, selector: u8) -> Result<Vec<u8>, Diagnostic> {
    let (iso_year, iso_week) = iso_week_year(
        parts.local.tm_year + 1900,
        parts.local.tm_yday + 1,
        parts.local.tm_wday,
    );
    match selector {
        b'g' => Ok(format!("{:02}", iso_year % 100).into_bytes()),
        b'G' => Ok(format!("{iso_year:04}").into_bytes()),
        b'V' => Ok(format!("{iso_week:02}").into_bytes()),
        _ => unreachable!("caller restricts selector"),
    }
}

fn hour_12(hour: i32) -> i32 {
    match hour % 12 {
        0 => 12,
        other => other,
    }
}

fn am_pm(hour: i32) -> &'static str {
    if hour < 12 { "AM" } else { "PM" }
}

fn seconds_without_fraction(second: i32) -> String {
    format!("{second:02}")
}

fn weekday_monday_one(wday: i32) -> i32 {
    if wday == 0 { 7 } else { wday }
}

fn render_week_number(parts: &ResolvedTimeParts, selector: u8) -> String {
    let week = match selector {
        b'U' => week_number_sunday(parts.local.tm_yday + 1, parts.local.tm_wday),
        b'W' => week_number_monday(parts.local.tm_yday + 1, parts.local.tm_wday),
        _ => unreachable!("caller restricts selector"),
    };
    format!("{week:02}")
}

fn week_number_sunday(yday_one_based: i32, wday: i32) -> i32 {
    ((yday_one_based + 6 - wday) / 7).max(0)
}

fn week_number_monday(yday_one_based: i32, wday: i32) -> i32 {
    let monday_index = weekday_monday_one(wday) - 1;
    ((yday_one_based + 6 - monday_index) / 7).max(0)
}

fn iso_week_year(year: i32, yday_one_based: i32, wday: i32) -> (i32, i32) {
    let monday_based_weekday = weekday_monday_one(wday);
    let thursday_yday = yday_one_based + (4 - monday_based_weekday);
    if thursday_yday < 1 {
        let prev_year = year - 1;
        let prev_year_len = if is_leap_year(prev_year) { 366 } else { 365 };
        return iso_week_year(prev_year, prev_year_len + thursday_yday, wday);
    }

    let year_len = if is_leap_year(year) { 366 } else { 365 };
    if thursday_yday > year_len {
        return iso_week_year(year + 1, thursday_yday - year_len, wday);
    }

    (year, ((thursday_yday - 1) / 7) + 1)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[allow(dead_code)]
fn timezone_name_bytes(local: &libc::tm) -> Vec<u8> {
    #[cfg(windows)]
    {
        let _ = local;
        return Vec::new();
    }

    #[cfg(unix)]
    {
        strftime_bytes(local, b"%Z\0").unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResolvedTimeParts, parse_numeric_utc_offset, render_full_time_bytes, render_selector_bytes,
    };
    use crate::printf::PrintfTimeSelector;
    use crate::time::Timestamp;

    fn sample_parts() -> ResolvedTimeParts {
        ResolvedTimeParts::for_tests(
            Timestamp::new(1_709_528_767, 123_456_789),
            2024,
            3,
            4,
            13,
            6,
            7,
            1,
            64,
            1,
            8 * 3600,
            b"CST".to_vec(),
        )
    }

    #[test]
    fn renders_gnu_special_epoch_selector_with_ten_fraction_digits() {
        let parts = sample_parts();
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::EpochSeconds).unwrap(),
            b"1709528767.1234567890"
        );
    }

    #[test]
    fn renders_gnu_special_plus_selector_in_local_time() {
        let parts = sample_parts();
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::GnuPlus).unwrap(),
            b"2024-03-04+13:06:07.1234567890"
        );
    }

    #[test]
    fn renders_textual_selectors_with_c_locale_spellings() {
        let parts = sample_parts();
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::Byte(b'a')).unwrap(),
            b"Mon"
        );
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::Byte(b'B')).unwrap(),
            b"March"
        );
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::Byte(b'p')).unwrap(),
            b"PM"
        );
    }

    #[test]
    fn renders_seconds_selector_and_full_directive_with_fractional_seconds() {
        let parts = sample_parts();
        assert_eq!(
            render_selector_bytes(&parts, PrintfTimeSelector::Byte(b'S')).unwrap(),
            b"07.1234567890"
        );
        assert_eq!(
            render_full_time_bytes(&parts).unwrap(),
            b"Mon Mar  4 13:06:07.1234567890 2024"
        );
    }

    #[cfg(unix)]
    #[test]
    fn parses_numeric_utc_offsets_from_strftime_forms() {
        assert_eq!(parse_numeric_utc_offset(b"+0800"), Some(8 * 3600));
        assert_eq!(parse_numeric_utc_offset(b"-0130"), Some(-(3600 + 30 * 60)));
        assert_eq!(parse_numeric_utc_offset(b"+08:00"), Some(8 * 3600));
        assert_eq!(parse_numeric_utc_offset(b"UTC"), None);
    }
}
