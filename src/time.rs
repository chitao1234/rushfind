use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
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
}

impl RelativeTimeMatcher {
    pub const fn new(
        kind: TimestampKind,
        unit: RelativeTimeUnit,
        comparison: TimeComparison,
        baseline: Timestamp,
    ) -> Self {
        Self {
            kind,
            unit,
            comparison,
            baseline,
        }
    }

    pub fn matches_timestamp(self, actual: Timestamp) -> bool {
        let bucket =
            (self.baseline.total_nanos() - actual.total_nanos()) / self.unit.bucket_nanos();

        match self.comparison {
            TimeComparison::Exactly(expected) => bucket == expected as i128,
            TimeComparison::LessThan(expected) => bucket < expected as i128,
            TimeComparison::GreaterThan(expected) => bucket > expected as i128,
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

pub fn parse_relative_time_argument(
    flag: &str,
    value: &OsStr,
    kind: TimestampKind,
    unit: RelativeTimeUnit,
    baseline: Timestamp,
) -> Result<RelativeTimeMatcher, Diagnostic> {
    Ok(RelativeTimeMatcher::new(
        kind,
        unit,
        parse_time_comparison(flag, value)?,
        baseline,
    ))
}

pub fn local_day_start(now: Timestamp) -> Result<Timestamp, Diagnostic> {
    let raw = now.seconds as libc::time_t;
    let mut local = MaybeUninit::<libc::tm>::uninit();
    let ptr = unsafe { libc::localtime_r(&raw, local.as_mut_ptr()) };
    if ptr.is_null() {
        return Err(Diagnostic::new(
            "failed to resolve local time for -daystart",
            1,
        ));
    }

    let mut local = unsafe { local.assume_init() };
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

pub fn resolve_reference_matcher(
    flag: &str,
    current: char,
    reference: char,
    reference_path: &Path,
    follow_mode: FollowMode,
) -> Result<NewerMatcher, Diagnostic> {
    let current = parse_current_timestamp_kind(flag, current)?;
    let reference = parse_reference_timestamp_kind(flag, reference)?;
    let metadata = reference_metadata(reference_path, follow_mode)?;

    Ok(NewerMatcher {
        current,
        reference: timestamp_from_metadata(reference, &metadata),
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
        'c' => Ok(TimestampKind::Change),
        'm' => Ok(TimestampKind::Modification),
        'B' => Err(Diagnostic::unsupported(format!(
            "unsupported in stage 8: `{flag}` forms involving birth time"
        ))),
        't' => Err(Diagnostic::unsupported(format!(
            "unsupported in stage 8: `{flag}` forms involving literal time"
        ))),
        other => Err(Diagnostic::new(
            format!("invalid `-newerXY` current timestamp kind `{other}`"),
            1,
        )),
    }
}

fn parse_reference_timestamp_kind(flag: &str, value: char) -> Result<TimestampKind, Diagnostic> {
    match value {
        'a' => Ok(TimestampKind::Access),
        'c' => Ok(TimestampKind::Change),
        'm' => Ok(TimestampKind::Modification),
        'B' => Err(Diagnostic::unsupported(format!(
            "unsupported in stage 8: `{flag}` forms involving birth time"
        ))),
        't' => Err(Diagnostic::unsupported(format!(
            "unsupported in stage 8: `{flag}` forms involving literal time"
        ))),
        other => Err(Diagnostic::new(
            format!("invalid `-newerXY` reference timestamp kind `{other}`"),
            1,
        )),
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
