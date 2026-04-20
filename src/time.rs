use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::literal_time::parse_literal_time;
use crate::platform::filesystem::{FsPlatformReader, PlatformReader};
use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::path::Path;
use std::str::{self, FromStr};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    pub seconds: i64,
    pub nanos: i32,
}

impl Timestamp {
    pub const fn new(seconds: i64, nanos: i32) -> Self {
        Self { seconds, nanos }
    }

    pub fn from_system_time(time: SystemTime) -> Result<Self, Diagnostic> {
        let duration = time.duration_since(UNIX_EPOCH).map_err(|error| {
            Diagnostic::new(format!("current time is before unix epoch: {error}"), 1)
        })?;

        Ok(Self::new(
            duration.as_secs() as i64,
            duration.subsec_nanos() as i32,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampKind {
    Access,
    Birth,
    Change,
    Modification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTimeUnit {
    Minutes,
    Days,
}

impl RelativeTimeUnit {
    const fn seconds(self) -> i64 {
        match self {
            Self::Minutes => 60,
            Self::Days => 86_400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeAmount {
    digits: Box<[u8]>,
    scale: u32,
}

impl TimeAmount {
    fn integral_value(&self) -> Option<i64> {
        if self.scale != 0 {
            return None;
        }

        str::from_utf8(&self.digits).ok()?.parse::<i64>().ok()
    }
}

impl FromStr for TimeAmount {
    type Err = Diagnostic;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (whole, frac) = match raw.split_once('.') {
            Some((whole, frac)) => (whole, Some(frac)),
            None => (raw, None),
        };

        if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Diagnostic::parse("invalid time amount"));
        }
        if frac.is_some_and(|value| {
            value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            return Err(Diagnostic::parse("invalid time amount"));
        }

        let mut digits = whole.as_bytes().to_vec();
        let mut scale = frac.map(|value| value.len() as u32).unwrap_or(0);
        if let Some(frac) = frac {
            digits.extend_from_slice(frac.as_bytes());
        }

        while scale > 0 && digits.last() == Some(&b'0') {
            digits.pop();
            scale -= 1;
        }

        while digits.len() > 1 && digits.first() == Some(&b'0') {
            digits.remove(0);
        }

        if digits.is_empty() {
            digits.push(b'0');
        }

        Ok(Self {
            digits: digits.into_boxed_slice(),
            scale,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeComparison {
    Exactly(TimeAmount),
    LessThan(TimeAmount),
    GreaterThan(TimeAmount),
}

impl TimeComparison {
    fn integral_value(&self) -> Option<i64> {
        match self {
            Self::Exactly(amount) | Self::LessThan(amount) | Self::GreaterThan(amount) => {
                amount.integral_value()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeTimeMatcher {
    pub kind: TimestampKind,
    pub unit: RelativeTimeUnit,
    pub comparison: TimeComparison,
    pub baseline: Timestamp,
    pub daystart: bool,
}

impl RelativeTimeMatcher {
    pub fn new(
        kind: TimestampKind,
        unit: RelativeTimeUnit,
        comparison: TimeComparison,
        baseline: Timestamp,
        daystart: bool,
    ) -> Self {
        Self {
            kind,
            unit,
            comparison,
            baseline,
            daystart,
        }
    }

    pub fn matches_timestamp(&self, actual: Timestamp) -> bool {
        self.matches_timestamp_checked(actual)
            .expect("relative time comparison should be computable")
    }

    pub fn matches_timestamp_checked(&self, actual: Timestamp) -> Result<bool, Diagnostic> {
        if self.daystart
            && matches!(self.unit, RelativeTimeUnit::Days)
            && self.comparison.integral_value().is_some()
        {
            let baseline_day = local_calendar_day(self.baseline)?;
            let actual_day = local_calendar_day(actual)?;
            Ok(matches_calendar_day_window(
                &self.comparison,
                baseline_day - actual_day,
            ))
        } else {
            matches_time_window(
                &self.comparison,
                self.baseline,
                actual,
                self.unit.seconds(),
                self.comparison_shifts(),
            )
        }
    }

    fn comparison_shifts(&self) -> TimeComparisonShifts {
        if self.daystart && matches!(self.unit, RelativeTimeUnit::Days) {
            // `-daystart` day predicates are centered on local-midnight buckets,
            // not rolling 24-hour windows.
            TimeComparisonShifts {
                exact_shift_units: 1,
                less_adjustment_units: -1,
                greater_shift_units: 0,
            }
        } else {
            self.unit.rolling_window_shifts()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewerMatcher {
    pub current: TimestampKind,
    pub reference: Timestamp,
}

impl NewerMatcher {
    pub fn matches_timestamp(self, actual: Timestamp) -> bool {
        actual > self.reference
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedMatcher {
    pub comparison: TimeComparison,
}

impl UsedMatcher {
    pub fn matches(&self, atime: Timestamp, ctime: Timestamp) -> bool {
        if atime < ctime {
            return false;
        }

        matches_time_window(
            &self.comparison,
            atime,
            ctime,
            RelativeTimeUnit::Days.seconds(),
            TimeComparisonShifts {
                exact_shift_units: 1,
                less_adjustment_units: 0,
                greater_shift_units: 0,
            },
        )
        .expect("used time comparison should be computable")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimeComparisonShifts {
    exact_shift_units: i64,
    less_adjustment_units: i64,
    greater_shift_units: i64,
}

impl RelativeTimeUnit {
    const fn rolling_window_shifts(self) -> TimeComparisonShifts {
        match self {
            Self::Minutes => TimeComparisonShifts {
                exact_shift_units: 1,
                less_adjustment_units: 0,
                greater_shift_units: 0,
            },
            Self::Days => TimeComparisonShifts {
                exact_shift_units: 0,
                less_adjustment_units: 0,
                greater_shift_units: 1,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimestampBoundary {
    BeforeAll,
    Finite(Timestamp),
    AfterAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoundingMode {
    Floor,
    Ceil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuantizedOffset {
    Finite(Timestamp),
    Overflow,
}

fn matches_calendar_day_window(comparison: &TimeComparison, elapsed_days: i64) -> bool {
    let Some(expected) = comparison.integral_value() else {
        return false;
    };

    match comparison {
        TimeComparison::Exactly(_) => elapsed_days == expected,
        TimeComparison::LessThan(_) => elapsed_days < expected,
        TimeComparison::GreaterThan(_) => elapsed_days > expected,
    }
}

fn matches_time_window(
    comparison: &TimeComparison,
    baseline: Timestamp,
    actual: Timestamp,
    unit_seconds: i64,
    shifts: TimeComparisonShifts,
) -> Result<bool, Diagnostic> {
    match comparison {
        TimeComparison::Exactly(amount) => {
            let lower = threshold_boundary(
                baseline,
                amount,
                unit_seconds,
                -shifts.exact_shift_units,
                RoundingMode::Ceil,
            );
            let upper = threshold_boundary(
                baseline,
                amount,
                unit_seconds,
                1 - shifts.exact_shift_units,
                RoundingMode::Floor,
            );

            Ok(timestamp_le_boundary(actual, lower) && timestamp_gt_boundary(actual, upper))
        }
        TimeComparison::LessThan(amount) => {
            let boundary = threshold_boundary(
                baseline,
                amount,
                unit_seconds,
                shifts.less_adjustment_units,
                RoundingMode::Ceil,
            );
            Ok(timestamp_gt_boundary(actual, boundary))
        }
        TimeComparison::GreaterThan(amount) => {
            let boundary = threshold_boundary(
                baseline,
                amount,
                unit_seconds,
                shifts.greater_shift_units,
                RoundingMode::Floor,
            );
            Ok(timestamp_lt_boundary(actual, boundary))
        }
    }
}

fn threshold_boundary(
    baseline: Timestamp,
    amount: &TimeAmount,
    unit_seconds: i64,
    unit_adjustment: i64,
    rounding: RoundingMode,
) -> TimestampBoundary {
    let adjusted = shift_timestamp_by_units(baseline, unit_seconds, unit_adjustment);
    let TimestampBoundary::Finite(adjusted) = adjusted else {
        return adjusted;
    };

    shift_timestamp_by_offset(adjusted, amount.quantize_offset(unit_seconds, rounding))
}

fn shift_timestamp_by_units(
    timestamp: Timestamp,
    unit_seconds: i64,
    delta_units: i64,
) -> TimestampBoundary {
    let delta_seconds = match unit_seconds.checked_mul(delta_units) {
        Some(delta_seconds) => delta_seconds,
        None if delta_units.is_negative() => return TimestampBoundary::AfterAll,
        None => return TimestampBoundary::BeforeAll,
    };
    let shifted_seconds = match timestamp.seconds.checked_sub(delta_seconds) {
        Some(shifted_seconds) => shifted_seconds,
        None if delta_seconds.is_negative() => return TimestampBoundary::AfterAll,
        None => return TimestampBoundary::BeforeAll,
    };

    TimestampBoundary::Finite(Timestamp::new(shifted_seconds, timestamp.nanos))
}

fn shift_timestamp_by_offset(timestamp: Timestamp, offset: QuantizedOffset) -> TimestampBoundary {
    let QuantizedOffset::Finite(offset) = offset else {
        return TimestampBoundary::BeforeAll;
    };

    let mut shifted_seconds = match timestamp.seconds.checked_sub(offset.seconds) {
        Some(shifted_seconds) => shifted_seconds,
        None => return TimestampBoundary::BeforeAll,
    };
    let shifted_nanos = if timestamp.nanos >= offset.nanos {
        timestamp.nanos - offset.nanos
    } else {
        shifted_seconds = match shifted_seconds.checked_sub(1) {
            Some(shifted_seconds) => shifted_seconds,
            None => return TimestampBoundary::BeforeAll,
        };
        timestamp.nanos + 1_000_000_000 - offset.nanos
    };

    TimestampBoundary::Finite(Timestamp::new(shifted_seconds, shifted_nanos))
}

impl TimeAmount {
    fn quantize_offset(&self, unit_seconds: i64, rounding: RoundingMode) -> QuantizedOffset {
        let seconds_digits = multiply_decimal_digits(&self.digits, unit_seconds as u32);
        let nanos_digits = quantized_nanoseconds_digits(&seconds_digits, self.scale, rounding);
        quantized_offset_from_nanoseconds(&nanos_digits)
    }
}

fn multiply_decimal_digits(digits: &[u8], multiplier: u32) -> Vec<u8> {
    let mut carry = 0u32;
    let mut product = Vec::with_capacity(digits.len() + 5);

    for digit in digits.iter().rev() {
        let value = u32::from(*digit - b'0') * multiplier + carry;
        product.push((value % 10) as u8 + b'0');
        carry = value / 10;
    }

    while carry > 0 {
        product.push((carry % 10) as u8 + b'0');
        carry /= 10;
    }

    product.reverse();
    product
}

fn quantized_nanoseconds_digits(digits: &[u8], scale: u32, rounding: RoundingMode) -> Vec<u8> {
    if scale <= 9 {
        let mut quantized = digits.to_vec();
        quantized.extend(std::iter::repeat_n(b'0', (9 - scale) as usize));
        trim_leading_zero_digits(&mut quantized);
        return quantized;
    }

    let truncated = (scale - 9) as usize;
    if truncated >= digits.len() {
        return match rounding {
            RoundingMode::Floor => vec![b'0'],
            RoundingMode::Ceil if digits.iter().any(|digit| *digit != b'0') => vec![b'1'],
            RoundingMode::Ceil => vec![b'0'],
        };
    }

    let split = digits.len() - truncated;
    let mut quantized = digits[..split].to_vec();
    let remainder_nonzero = digits[split..].iter().any(|digit| *digit != b'0');
    trim_leading_zero_digits(&mut quantized);

    if matches!(rounding, RoundingMode::Ceil) && remainder_nonzero {
        increment_decimal_digits(&mut quantized);
    }

    quantized
}

fn quantized_offset_from_nanoseconds(digits: &[u8]) -> QuantizedOffset {
    let len = digits.len();
    let seconds_digits = if len > 9 { &digits[..len - 9] } else { b"0" };
    let seconds = match parse_nonnegative_i64_digits(seconds_digits) {
        Some(seconds) => seconds,
        None => return QuantizedOffset::Overflow,
    };

    let mut nanos_digits = [b'0'; 9];
    if len > 9 {
        nanos_digits.copy_from_slice(&digits[len - 9..]);
    } else {
        nanos_digits[9 - len..].copy_from_slice(digits);
    }

    QuantizedOffset::Finite(Timestamp::new(
        seconds,
        str::from_utf8(&nanos_digits)
            .expect("nanosecond digits are ascii")
            .parse::<i32>()
            .expect("nanosecond digits fit in i32"),
    ))
}

fn parse_nonnegative_i64_digits(digits: &[u8]) -> Option<i64> {
    let normalized = normalized_digit_slice(digits);
    let max = b"9223372036854775807";
    if normalized.len() > max.len()
        || (normalized.len() == max.len() && normalized > max.as_slice())
    {
        return None;
    }

    str::from_utf8(normalized).ok()?.parse::<i64>().ok()
}

fn normalized_digit_slice(digits: &[u8]) -> &[u8] {
    let index = digits
        .iter()
        .position(|digit| *digit != b'0')
        .unwrap_or(digits.len().saturating_sub(1));
    &digits[index..]
}

fn trim_leading_zero_digits(digits: &mut Vec<u8>) {
    while digits.len() > 1 && digits.first() == Some(&b'0') {
        digits.remove(0);
    }
}

fn increment_decimal_digits(digits: &mut Vec<u8>) {
    let mut carry = true;
    for digit in digits.iter_mut().rev() {
        if !carry {
            break;
        }

        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            carry = false;
        }
    }

    if carry {
        digits.insert(0, b'1');
    }
}

fn timestamp_gt_boundary(actual: Timestamp, boundary: TimestampBoundary) -> bool {
    match boundary {
        TimestampBoundary::BeforeAll => true,
        TimestampBoundary::Finite(boundary) => actual > boundary,
        TimestampBoundary::AfterAll => false,
    }
}

fn timestamp_lt_boundary(actual: Timestamp, boundary: TimestampBoundary) -> bool {
    match boundary {
        TimestampBoundary::BeforeAll => false,
        TimestampBoundary::Finite(boundary) => actual < boundary,
        TimestampBoundary::AfterAll => true,
    }
}

fn timestamp_le_boundary(actual: Timestamp, boundary: TimestampBoundary) -> bool {
    match boundary {
        TimestampBoundary::BeforeAll => false,
        TimestampBoundary::Finite(boundary) => actual <= boundary,
        TimestampBoundary::AfterAll => true,
    }
}

pub fn parse_relative_time_argument(
    flag: &str,
    value: &OsStr,
    kind: TimestampKind,
    unit: RelativeTimeUnit,
    baseline: Timestamp,
    daystart: bool,
) -> Result<RelativeTimeMatcher, Diagnostic> {
    Ok(RelativeTimeMatcher::new(
        kind,
        unit,
        parse_time_comparison(flag, value)?,
        baseline,
        daystart,
    ))
}

pub fn validate_time_argument(flag: &str, value: &OsStr) -> Result<(), Diagnostic> {
    parse_time_comparison(flag, value).map(|_| ())
}

pub fn local_day_start(now: Timestamp) -> Result<Timestamp, Diagnostic> {
    let mut local = local_time(now.seconds)?;
    local.tm_hour = 0;
    local.tm_min = 0;
    local.tm_sec = 0;
    local.tm_isdst = -1;

    let day_start = unsafe { libc::mktime(&mut local) };
    if day_start == -1 {
        return Err(Diagnostic::new("failed to compute local day start", 1));
    }

    Ok(Timestamp::new(day_start as i64, 0))
}

fn local_calendar_day(timestamp: Timestamp) -> Result<i64, Diagnostic> {
    let local = local_time(timestamp.seconds)?;
    Ok(days_from_civil(
        local.tm_year + 1900,
        (local.tm_mon + 1) as u32,
        local.tm_mday as u32,
    ))
}

fn local_time(seconds: i64) -> Result<libc::tm, Diagnostic> {
    let raw = seconds as libc::time_t;
    let mut local = MaybeUninit::<libc::tm>::uninit();
    let ptr = unsafe { libc::localtime_r(&raw, local.as_mut_ptr()) };
    if ptr.is_null() {
        return Err(Diagnostic::new(
            "failed to resolve local time for relative time matching",
            1,
        ));
    }

    Ok(unsafe { local.assume_init() })
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - (era * 400);
    let shifted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = ((153 * shifted_month) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    (era * 146_097 + day_of_era - 719_468) as i64
}

pub fn resolve_reference_matcher(
    flag: &str,
    current: char,
    reference: char,
    reference_arg: &OsStr,
    follow_mode: FollowMode,
) -> Result<NewerMatcher, Diagnostic> {
    let current = parse_current_timestamp_kind(flag, current)?;
    Ok(NewerMatcher {
        current,
        reference: resolve_reference_timestamp(flag, reference, reference_arg, follow_mode)?,
    })
}

pub fn parse_time_comparison(flag: &str, value: &OsStr) -> Result<TimeComparison, Diagnostic> {
    let bytes = value.as_encoded_bytes();
    let (kind, digits) = match bytes {
        [b'+', rest @ ..] => (ComparisonKind::GreaterThan, rest),
        [b'-', rest @ ..] => (ComparisonKind::LessThan, rest),
        _ => (ComparisonKind::Exactly, bytes),
    };

    let parsed = str::from_utf8(digits)
        .map_err(|_| invalid_numeric_argument(flag, value))?
        .parse::<TimeAmount>()
        .map_err(|_| invalid_numeric_argument(flag, value))?;

    Ok(match kind {
        ComparisonKind::Exactly => TimeComparison::Exactly(parsed),
        ComparisonKind::LessThan => TimeComparison::LessThan(parsed),
        ComparisonKind::GreaterThan => TimeComparison::GreaterThan(parsed),
    })
}

fn invalid_numeric_argument(flag: &str, value: &OsStr) -> Diagnostic {
    Diagnostic::parse(format!(
        "invalid numeric argument for `{flag}`: `{}`",
        value.to_string_lossy()
    ))
}

fn parse_current_timestamp_kind(flag: &str, value: char) -> Result<TimestampKind, Diagnostic> {
    match value {
        'a' => Ok(TimestampKind::Access),
        'B' => Ok(TimestampKind::Birth),
        'c' => Ok(TimestampKind::Change),
        'm' => Ok(TimestampKind::Modification),
        't' => Err(Diagnostic::new(
            format!("invalid `-newerXY` current timestamp kind `t` in `{flag}`"),
            1,
        )),
        other => Err(Diagnostic::new(
            format!("invalid `-newerXY` current timestamp kind `{other}`"),
            1,
        )),
    }
}

fn resolve_reference_timestamp(
    flag: &str,
    reference: char,
    reference_arg: &OsStr,
    follow_mode: FollowMode,
) -> Result<Timestamp, Diagnostic> {
    let reader = FsPlatformReader;
    match reference {
        'a' => reference_metadata_timestamp(
            &reader,
            Path::new(reference_arg),
            follow_mode,
            TimestampKind::Access,
        ),
        'B' => reference_metadata_timestamp(
            &reader,
            Path::new(reference_arg),
            follow_mode,
            TimestampKind::Birth,
        )
        .map_err(|error| {
            if error.message.contains("birth time is not available") {
                Diagnostic::new(
                    format!(
                        "reference birth time unavailable for `{}` in `{flag}`",
                        Path::new(reference_arg).display()
                    ),
                    1,
                )
            } else {
                error
            }
        }),
        'c' => reference_metadata_timestamp(
            &reader,
            Path::new(reference_arg),
            follow_mode,
            TimestampKind::Change,
        ),
        'm' => reference_metadata_timestamp(
            &reader,
            Path::new(reference_arg),
            follow_mode,
            TimestampKind::Modification,
        ),
        't' => parse_literal_time(reference_arg),
        other => Err(Diagnostic::new(
            format!("invalid `-newerXY` reference timestamp kind `{other}`"),
            1,
        )),
    }
}

fn reference_metadata_timestamp(
    reader: &dyn PlatformReader,
    path: &Path,
    follow_mode: FollowMode,
    kind: TimestampKind,
) -> Result<Timestamp, Diagnostic> {
    let view = match follow_mode {
        FollowMode::Physical => reader.metadata_view(path, false),
        FollowMode::CommandLineOnly | FollowMode::Logical => reader
            .metadata_view(path, true)
            .or_else(|_| reader.metadata_view(path, false)),
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;

    match kind {
        TimestampKind::Access => Ok(view.atime),
        TimestampKind::Birth => view.birth_time.ok_or_else(|| {
            Diagnostic::new(
                format!("{}: birth time is not available", path.display()),
                1,
            )
        }),
        TimestampKind::Change => Ok(view.ctime),
        TimestampKind::Modification => Ok(view.mtime),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonKind {
    Exactly,
    LessThan,
    GreaterThan,
}
