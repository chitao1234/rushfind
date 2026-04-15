use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::numeric::NumericComparison;
use findoxide::output::RecordingSink;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
use findoxide::size::parse_size_argument;
use findoxide::time::{
    NewerMatcher, RelativeTimeMatcher, RelativeTimeUnit, TimeComparison, Timestamp, TimestampKind,
    UsedMatcher, local_day_start,
};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn size_uses_gnu_rounded_up_unit_semantics() {
    let root = tempdir().unwrap();
    let empty = root.path().join("empty.bin");
    let one_byte = root.path().join("one-byte.bin");
    let five_thirteen = root.path().join("five-thirteen.bin");
    fs::write(&empty, []).unwrap();
    fs::write(&one_byte, [b'x']).unwrap();
    fs::write(&five_thirteen, vec![b'x'; 513]).unwrap();

    assert!(evaluate_size(&empty, "-1M", FollowMode::Physical));
    assert!(!evaluate_size(&one_byte, "-1M", FollowMode::Physical));
    assert!(!evaluate_size(&five_thirteen, "1b", FollowMode::Physical));
    assert!(evaluate_size(&five_thirteen, "2b", FollowMode::Physical));
}

#[test]
fn size_reads_the_active_follow_mode_view() {
    let root = tempdir().unwrap();
    let target = root.path().join("target.bin");
    let link = root.path().join("target-link");
    fs::write(&target, vec![b'x'; 2049]).unwrap();
    unix_fs::symlink("target.bin", &link).unwrap();

    let physical_len = std::fs::symlink_metadata(&link).unwrap().len();
    let logical_len = std::fs::metadata(&link).unwrap().len();

    assert!(evaluate_size(
        &link,
        &format!("{physical_len}c"),
        FollowMode::Physical,
    ));
    assert!(evaluate_size(
        &link,
        &format!("{logical_len}c"),
        FollowMode::Logical,
    ));
}

#[test]
fn minute_relative_time_matcher_uses_gnu_shifted_windows() {
    let now = Timestamp::new(10_000, 0);
    let fresh = Timestamp::new(10_000, 0);
    let thirty_seconds_old = Timestamp::new(9_970, 0);
    let sixty_seconds_old = Timestamp::new(9_940, 0);
    let sixty_one_seconds_old = Timestamp::new(9_939, 0);

    let exact_zero = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::Exactly(0),
        now,
        false,
    );
    let exact_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::Exactly(1),
        now,
        false,
    );
    let less_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::LessThan(1),
        now,
        false,
    );
    let greater_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::GreaterThan(1),
        now,
        false,
    );

    assert!(!exact_zero.matches_timestamp(fresh));
    assert!(!exact_zero.matches_timestamp(thirty_seconds_old));
    assert!(exact_one.matches_timestamp(fresh));
    assert!(exact_one.matches_timestamp(thirty_seconds_old));
    assert!(!exact_one.matches_timestamp(sixty_seconds_old));
    assert!(less_than_one.matches_timestamp(fresh));
    assert!(less_than_one.matches_timestamp(thirty_seconds_old));
    assert!(!less_than_one.matches_timestamp(sixty_seconds_old));
    assert!(!greater_than_one.matches_timestamp(sixty_seconds_old));
    assert!(greater_than_one.matches_timestamp(sixty_one_seconds_old));
}

#[test]
fn minute_relative_time_matcher_preserves_subsecond_boundaries() {
    let now = Timestamp::new(10_000, 100_000_000);
    let just_under_one_minute = Timestamp::new(9_940, 200_000_000);
    let just_over_one_minute = Timestamp::new(9_940, 0);

    let exact_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::Exactly(1),
        now,
        false,
    );
    let less_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::LessThan(1),
        now,
        false,
    );
    let greater_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::GreaterThan(1),
        now,
        false,
    );

    assert!(exact_one.matches_timestamp(just_under_one_minute));
    assert!(!exact_one.matches_timestamp(just_over_one_minute));
    assert!(less_than_one.matches_timestamp(just_under_one_minute));
    assert!(!less_than_one.matches_timestamp(just_over_one_minute));
    assert!(!greater_than_one.matches_timestamp(just_under_one_minute));
    assert!(greater_than_one.matches_timestamp(just_over_one_minute));
}

#[test]
fn day_relative_time_matcher_uses_gnu_day_windows() {
    let now = Timestamp::new(200_000, 0);
    let fresh = Timestamp::new(199_999, 0);
    let almost_one_day_old = Timestamp::new(200_000 - 86_399, 0);
    let exactly_one_day_old = Timestamp::new(200_000 - 86_400, 0);
    let almost_two_days_old = Timestamp::new(200_000 - 172_799, 0);
    let exactly_two_days_old = Timestamp::new(200_000 - 172_800, 0);
    let more_than_two_days_old = Timestamp::new(200_000 - 172_801, 0);

    let exact_zero = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Days,
        TimeComparison::Exactly(0),
        now,
        false,
    );
    let exact_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Days,
        TimeComparison::Exactly(1),
        now,
        false,
    );
    let less_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Days,
        TimeComparison::LessThan(1),
        now,
        false,
    );
    let greater_than_one = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Days,
        TimeComparison::GreaterThan(1),
        now,
        false,
    );

    assert!(exact_zero.matches_timestamp(fresh));
    assert!(exact_zero.matches_timestamp(almost_one_day_old));
    assert!(!exact_zero.matches_timestamp(exactly_one_day_old));
    assert!(!exact_zero.matches_timestamp(almost_two_days_old));
    assert!(!exact_one.matches_timestamp(almost_one_day_old));
    assert!(exact_one.matches_timestamp(exactly_one_day_old));
    assert!(exact_one.matches_timestamp(almost_two_days_old));
    assert!(!exact_one.matches_timestamp(exactly_two_days_old));
    assert!(less_than_one.matches_timestamp(fresh));
    assert!(less_than_one.matches_timestamp(almost_one_day_old));
    assert!(!less_than_one.matches_timestamp(exactly_one_day_old));
    assert!(!greater_than_one.matches_timestamp(almost_two_days_old));
    assert!(!greater_than_one.matches_timestamp(exactly_two_days_old));
    assert!(greater_than_one.matches_timestamp(more_than_two_days_old));
}

#[test]
fn used_matcher_matches_gnu_used_windows() {
    let exact_zero = UsedMatcher {
        comparison: NumericComparison::Exactly(0),
    };
    let exact_one = UsedMatcher {
        comparison: NumericComparison::Exactly(1),
    };
    let greater_than_one = UsedMatcher {
        comparison: NumericComparison::GreaterThan(1),
    };
    let less_than_one = UsedMatcher {
        comparison: NumericComparison::LessThan(1),
    };

    assert!(!exact_zero.matches(Timestamp::new(1, 0), Timestamp::new(0, 0)));
    assert!(exact_one.matches(Timestamp::new(1, 0), Timestamp::new(0, 0)));
    assert!(!exact_one.matches(Timestamp::new(86_400, 0), Timestamp::new(0, 0)));
    assert!(greater_than_one.matches(Timestamp::new(86_401, 0), Timestamp::new(0, 0)));
    assert!(less_than_one.matches(Timestamp::new(1, 0), Timestamp::new(0, 0)));
    assert!(!less_than_one.matches(Timestamp::new(0, 0), Timestamp::new(1, 0)));
}

#[test]
fn daystart_day_matching_uses_calendar_day_boundaries() {
    let daystart = local_day_start(Timestamp::new(1_700_000_000, 0)).unwrap();
    let exact_today = RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Days,
        TimeComparison::Exactly(0),
        daystart,
        true,
    );

    assert!(exact_today.matches_timestamp(Timestamp::new(daystart.seconds + 1, 0)));
    assert!(!exact_today.matches_timestamp(Timestamp::new(daystart.seconds - 1, 0)));
}

#[test]
fn relative_time_evaluation_reads_the_active_follow_mode_timestamp() {
    let root = tempdir().unwrap();
    let target = root.path().join("target.bin");
    let link = root.path().join("target-link");
    fs::write(&target, b"hello\n").unwrap();
    unix_fs::symlink("target.bin", &link).unwrap();

    set_file_times(
        &target,
        Timestamp::new(1_699_999_700, 0),
        Timestamp::new(1_699_999_700, 0),
    );

    let expr = RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(RelativeTimeMatcher::new(
        TimestampKind::Modification,
        RelativeTimeUnit::Minutes,
        TimeComparison::GreaterThan(1),
        Timestamp::new(1_700_000_000, 0),
        false,
    )));
    let entry = EntryContext::new(link, 0, true);
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
}

#[test]
fn newer_matcher_compares_full_timestamp_precision() {
    let root = tempdir().unwrap();
    let older = root.path().join("older.txt");
    let newer = root.path().join("newer.txt");
    fs::write(&older, "older\n").unwrap();
    fs::write(&newer, "newer\n").unwrap();
    set_file_times(&older, Timestamp::new(100, 10), Timestamp::new(100, 10));
    set_file_times(&newer, Timestamp::new(100, 20), Timestamp::new(100, 20));

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Newer(NewerMatcher {
        current: TimestampKind::Modification,
        reference: Timestamp::new(100, 10),
    }));
    let mut sink = RecordingSink::default();

    assert!(
        !evaluate(
            &expr,
            &EntryContext::new(older, 0, true),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &expr,
            &EntryContext::new(newer, 0, true),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

fn evaluate_size(path: &Path, raw: &str, follow_mode: FollowMode) -> bool {
    let entry = EntryContext::new(PathBuf::from(path), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Size(
        parse_size_argument(OsStr::new(raw)).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    evaluate(&expr, &entry, follow_mode, &mut sink).unwrap()
}

fn set_file_times(path: &Path, atime: Timestamp, mtime: Timestamp) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: atime.seconds as libc::time_t,
            tv_nsec: atime.nanos.into(),
        },
        libc::timespec {
            tv_sec: mtime.seconds as libc::time_t,
            tv_nsec: mtime.nanos.into(),
        },
    ];

    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0);
}
