use crate::birth::read_birth_time;
use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::literal_time::parse_literal_time;
use std::ffi::OsStr;
use std::fs::{self, Metadata};
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
        if frac.is_some_and(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())) {
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
        let comparison = WindowComparison::try_from(&self.comparison)?;
        if self.daystart && matches!(self.unit, RelativeTimeUnit::Days) {
            let baseline_day = local_calendar_day(self.baseline)?;
            let actual_day = local_calendar_day(actual)?;
            Ok(matches_calendar_day_window(comparison, baseline_day - actual_day))
        } else {
            Ok(matches_timestamp_window(
                comparison,
                self.baseline,
                actual,
                self.unit.window(),
            ))
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

        let Ok(comparison) = WindowComparison::try_from(&self.comparison) else {
            return false;
        };

        matches_timestamp_window(
            comparison,
            atime,
            ctime,
            WholeSecondWindow::shifted_exact(RelativeTimeUnit::Days.seconds()),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WholeSecondWindow {
    unit_seconds: i64,
    exact_shift_units: i64,
    greater_shift_units: i64,
}

impl WholeSecondWindow {
    const fn new(unit_seconds: i64, exact_shift_units: i64, greater_shift_units: i64) -> Self {
        Self {
            unit_seconds,
            exact_shift_units,
            greater_shift_units,
        }
    }

    const fn shifted_exact(unit_seconds: i64) -> Self {
        Self::new(unit_seconds, 1, 0)
    }

    const fn elapsed(unit_seconds: i64) -> Self {
        Self::new(unit_seconds, 0, 1)
    }
}

impl RelativeTimeUnit {
    const fn window(self) -> WholeSecondWindow {
        match self {
            Self::Minutes => WholeSecondWindow::shifted_exact(Self::Minutes.seconds()),
            Self::Days => WholeSecondWindow::elapsed(Self::Days.seconds()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowComparison {
    Exactly(WindowExpected),
    LessThan(WindowExpected),
    GreaterThan(WindowExpected),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowExpected {
    Finite(i64),
}

impl TryFrom<&TimeComparison> for WindowComparison {
    type Error = Diagnostic;

    fn try_from(value: &TimeComparison) -> Result<Self, Self::Error> {
        let Some(expected) = value.integral_value() else {
            return Err(Diagnostic::new(
                "fractional time comparisons are not yet implemented",
                1,
            ));
        };

        Ok(match value {
            TimeComparison::Exactly(_) => Self::Exactly(WindowExpected::Finite(expected)),
            TimeComparison::LessThan(_) => Self::LessThan(WindowExpected::Finite(expected)),
            TimeComparison::GreaterThan(_) => Self::GreaterThan(WindowExpected::Finite(expected)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimestampBoundary {
    BeforeAll,
    Finite(Timestamp),
    AfterAll,
}

fn matches_calendar_day_window(comparison: WindowComparison, elapsed_days: i64) -> bool {
    match comparison {
        WindowComparison::Exactly(WindowExpected::Finite(expected)) => elapsed_days == expected,
        WindowComparison::LessThan(WindowExpected::Finite(expected)) => elapsed_days < expected,
        WindowComparison::GreaterThan(WindowExpected::Finite(expected)) => elapsed_days > expected,
    }
}

fn matches_timestamp_window(
    comparison: WindowComparison,
    baseline: Timestamp,
    actual: Timestamp,
    window: WholeSecondWindow,
) -> bool {
    match comparison {
        WindowComparison::Exactly(WindowExpected::Finite(expected)) => {
            let lower = shift_timestamp_boundary(
                baseline,
                window.unit_seconds,
                expected,
                window.exact_shift_units,
            );
            let upper = shift_timestamp_boundary(
                baseline,
                window.unit_seconds,
                expected,
                window.exact_shift_units - 1,
            );

            timestamp_le_boundary(actual, lower) && timestamp_gt_boundary(actual, upper)
        }
        WindowComparison::LessThan(WindowExpected::Finite(expected)) => {
            let boundary = shift_timestamp_boundary(baseline, window.unit_seconds, expected, 0);
            timestamp_gt_boundary(actual, boundary)
        }
        WindowComparison::GreaterThan(WindowExpected::Finite(expected)) => {
            let boundary = shift_timestamp_boundary(
                baseline,
                window.unit_seconds,
                expected,
                -window.greater_shift_units,
            );
            timestamp_lt_boundary(actual, boundary)
        }
    }
}

fn shift_timestamp_boundary(
    baseline: Timestamp,
    unit_seconds: i64,
    expected_units: i64,
    boundary_units: i64,
) -> TimestampBoundary {
    let delta_units = match boundary_units.checked_sub(expected_units) {
        Some(delta_units) => delta_units,
        None if expected_units.is_negative() => return TimestampBoundary::AfterAll,
        None => return TimestampBoundary::BeforeAll,
    };

    shift_timestamp_units(baseline, unit_seconds, delta_units)
}

fn shift_timestamp_units(
    timestamp: Timestamp,
    unit_seconds: i64,
    delta_units: i64,
) -> TimestampBoundary {
    let quotient = timestamp.seconds.div_euclid(unit_seconds);
    let remainder = timestamp.seconds.rem_euclid(unit_seconds);
    let shifted_quotient = match quotient.checked_add(delta_units) {
        Some(shifted_quotient) => shifted_quotient,
        None if delta_units.is_negative() => return TimestampBoundary::BeforeAll,
        None => return TimestampBoundary::AfterAll,
    };
    let shifted_seconds = match shifted_quotient
        .checked_mul(unit_seconds)
        .and_then(|seconds| seconds.checked_add(remainder))
    {
        Some(shifted_seconds) => shifted_seconds,
        None if shifted_quotient.is_negative() => return TimestampBoundary::BeforeAll,
        None => return TimestampBoundary::AfterAll,
    };

    TimestampBoundary::Finite(Timestamp::new(shifted_seconds, timestamp.nanos))
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
