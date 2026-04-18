use crate::diagnostics::Diagnostic;
use crate::printf::PrintfTimeSelector;
use crate::time::Timestamp;
use std::ffi::CStr;
use std::mem::MaybeUninit;

const WEEKDAYS_ABBR: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
const MONTHS_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
    b"Dec",
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

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTimeParts {
    pub timestamp: Timestamp,
    pub local: libc::tm,
    pub timezone_name: Vec<u8>,
    pub utc_offset_seconds: i32,
}

impl ResolvedTimeParts {
    #[cfg(test)]
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
            local: libc::tm {
                tm_sec: second,
                tm_min: minute,
                tm_hour: hour,
                tm_mday: day,
                tm_mon: month - 1,
                tm_year: year - 1900,
                tm_wday: weekday,
                tm_yday: yearday - 1,
                tm_isdst: is_dst,
                tm_gmtoff: utc_offset_seconds as libc::c_long,
                tm_zone: std::ptr::null(),
            },
            timezone_name,
            utc_offset_seconds,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_local_time_parts(timestamp: Timestamp) -> Result<ResolvedTimeParts, Diagnostic> {
    let raw = timestamp.seconds as libc::time_t;
    let mut local = MaybeUninit::<libc::tm>::uninit();
    let ptr = unsafe { libc::localtime_r(&raw, local.as_mut_ptr()) };
    if ptr.is_null() {
        return Err(Diagnostic::new("failed to resolve local time for -printf", 1));
    }

    let local = unsafe { local.assume_init() };
    Ok(ResolvedTimeParts {
        timestamp,
        timezone_name: timezone_name_bytes(&local),
        utc_offset_seconds: local.tm_gmtoff as i32,
        local,
    })
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
            b'B' => Ok(MONTHS_FULL[parts.local.tm_mon as usize].to_vec()),
            b'p' => Ok(if parts.local.tm_hour < 12 {
                b"AM".to_vec()
            } else {
                b"PM".to_vec()
            }),
            b'S' => Ok(seconds_with_fraction(parts.local.tm_sec, parts.timestamp.nanos).into()),
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

#[allow(dead_code)]
fn timezone_name_bytes(local: &libc::tm) -> Vec<u8> {
    if local.tm_zone.is_null() {
        return Vec::new();
    }

    unsafe { CStr::from_ptr(local.tm_zone) }.to_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::{ResolvedTimeParts, render_full_time_bytes, render_selector_bytes};
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
}
