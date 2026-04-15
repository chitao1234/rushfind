use crate::birth::read_birth_time;
use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::literal_time::parse_literal_time;
use crate::numeric::NumericComparison;
use std::ffi::OsStr;
use std::fs::{self, Metadata};
use std::mem::MaybeUninit;
use std::path::Path;
use std::str;
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

    fn total_nanos(self) -> i128 {
        (self.seconds as i128 * 1_000_000_000) + self.nanos as i128
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
    fn bucket_nanos(self) -> i128 {
        match self {
            Self::Minutes => 60 * 1_000_000_000,
            Self::Days => 86_400 * 1_000_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeComparison {
    Exactly(i64),
    LessThan(i64),
    GreaterThan(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeTimeMatcher {
    pub kind: TimestampKind,
    pub unit: RelativeTimeUnit,
    pub comparison: TimeComparison,
    pub baseline: Timestamp,
    pub daystart: bool,
}

impl RelativeTimeMatcher {
    pub const fn new(
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

    pub fn matches_timestamp(self, actual: Timestamp) -> bool {
        self.matches_timestamp_checked(actual)
            .expect("relative time comparison should be computable")
    }

    pub fn matches_timestamp_checked(self, actual: Timestamp) -> Result<bool, Diagnostic> {
        if matches!(self.unit, RelativeTimeUnit::Minutes) {
            return Ok(self.matches_minute_timestamp(actual));
        }

        let bucket = self.bucket(actual)?;

        Ok(match self.comparison {
            TimeComparison::Exactly(expected) => bucket == expected as i128,
            TimeComparison::LessThan(expected) => bucket < expected as i128,
            TimeComparison::GreaterThan(expected) => bucket > expected as i128,
        })
    }

    fn bucket(self, actual: Timestamp) -> Result<i128, Diagnostic> {
        if self.daystart && matches!(self.unit, RelativeTimeUnit::Days) {
            let baseline_day = local_calendar_day(self.baseline)?;
            let actual_day = local_calendar_day(actual)?;
            Ok((baseline_day - actual_day) as i128)
        } else {
            Ok((self.baseline.total_nanos() - actual.total_nanos()) / self.unit.bucket_nanos())
        }
    }

    fn matches_minute_timestamp(self, actual: Timestamp) -> bool {
        let elapsed = self.baseline.total_nanos() - actual.total_nanos();
        let minute = RelativeTimeUnit::Minutes.bucket_nanos();

        match self.comparison {
            TimeComparison::Exactly(expected) => {
                let expected = expected as i128;
                elapsed >= (expected - 1) * minute && elapsed < expected * minute
            }
            TimeComparison::LessThan(expected) => elapsed < expected as i128 * minute,
            TimeComparison::GreaterThan(expected) => elapsed > expected as i128 * minute,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsedMatcher {
    pub comparison: NumericComparison,
}

impl UsedMatcher {
    pub fn matches(self, atime: Timestamp, ctime: Timestamp) -> bool {
        if atime < ctime {
            return false;
        }

        let elapsed = atime.total_nanos() - ctime.total_nanos();
        let day = RelativeTimeUnit::Days.bucket_nanos();

        match self.comparison {
            NumericComparison::Exactly(expected) => {
                let expected = expected as i128;
                elapsed >= (expected - 1) * day && elapsed < expected * day
            }
            NumericComparison::LessThan(expected) => elapsed < expected as i128 * day,
            NumericComparison::GreaterThan(expected) => elapsed > expected as i128 * day,
        }
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

fn parse_time_comparison(flag: &str, value: &OsStr) -> Result<TimeComparison, Diagnostic> {
    let bytes = value.as_encoded_bytes();
    let (kind, digits) = match bytes {
        [b'+', rest @ ..] => (ComparisonKind::GreaterThan, rest),
        [b'-', rest @ ..] => (ComparisonKind::LessThan, rest),
        _ => (ComparisonKind::Exactly, bytes),
    };

    if digits.is_empty() || !digits.iter().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_numeric_argument(flag, value));
    }

    let parsed = str::from_utf8(digits)
        .map_err(|_| invalid_numeric_argument(flag, value))?
        .parse::<i64>()
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
    match reference {
        'a' => {
            let metadata = reference_metadata(Path::new(reference_arg), follow_mode)?;
            Ok(timestamp_from_metadata(TimestampKind::Access, &metadata))
        }
        'B' => {
            let path = Path::new(reference_arg);
            resolve_reference_birth_time(path, follow_mode)?.ok_or_else(|| {
                Diagnostic::new(
                    format!(
                        "reference birth time unavailable for `{}` in `{flag}`",
                        path.display()
                    ),
                    1,
                )
            })
        }
        'c' => {
            let metadata = reference_metadata(Path::new(reference_arg), follow_mode)?;
            Ok(timestamp_from_metadata(TimestampKind::Change, &metadata))
        }
        'm' => {
            let metadata = reference_metadata(Path::new(reference_arg), follow_mode)?;
            Ok(timestamp_from_metadata(
                TimestampKind::Modification,
                &metadata,
            ))
        }
        't' => parse_literal_time(reference_arg),
        other => Err(Diagnostic::new(
            format!("invalid `-newerXY` reference timestamp kind `{other}`"),
            1,
        )),
    }
}

fn resolve_reference_birth_time(
    path: &Path,
    follow_mode: FollowMode,
) -> Result<Option<Timestamp>, Diagnostic> {
    match follow_mode {
        FollowMode::Physical => read_birth_time(path, false),
        FollowMode::CommandLineOnly | FollowMode::Logical => match read_birth_time(path, true) {
            Ok(timestamp) => Ok(timestamp),
            Err(_) => read_birth_time(path, false),
        },
    }
}

fn reference_metadata(path: &Path, follow_mode: FollowMode) -> Result<Metadata, Diagnostic> {
    match follow_mode {
        FollowMode::Physical => fs::symlink_metadata(path),
        FollowMode::CommandLineOnly | FollowMode::Logical => {
            fs::metadata(path).or_else(|_| fs::symlink_metadata(path))
        }
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))
}

fn timestamp_from_metadata(kind: TimestampKind, metadata: &Metadata) -> Timestamp {
    use std::os::unix::fs::MetadataExt;

    match kind {
        TimestampKind::Access => Timestamp::new(metadata.atime(), metadata.atime_nsec() as i32),
        TimestampKind::Birth => unreachable!("birth timestamps are resolved via statx"),
        TimestampKind::Change => Timestamp::new(metadata.ctime(), metadata.ctime_nsec() as i32),
        TimestampKind::Modification => {
            Timestamp::new(metadata.mtime(), metadata.mtime_nsec() as i32)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonKind {
    Exactly,
    LessThan,
    GreaterThan,
}
