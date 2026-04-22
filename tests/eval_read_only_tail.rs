#![cfg(unix)]

mod support;

use rushfind::birth::read_birth_time;
use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::output::RecordingSink;
use rushfind::planner::{RuntimeExpr, RuntimePredicate};
use rushfind::time::{NewerMatcher, TimeComparison, Timestamp, TimestampKind, UsedMatcher};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{self as unix_fs, MetadataExt};
use std::path::Path;
use tempfile::tempdir;

#[test]
fn empty_matches_only_empty_regular_files_and_directories() {
    let root = tempdir().unwrap();
    let empty_dir = root.path().join("empty-dir");
    let nonempty_dir = root.path().join("nonempty-dir");
    let empty_file = root.path().join("empty-file");
    let nonempty_file = root.path().join("nonempty-file");
    fs::create_dir(&empty_dir).unwrap();
    fs::create_dir(&nonempty_dir).unwrap();
    fs::write(nonempty_dir.join("child"), "child\n").unwrap();
    fs::write(&empty_file, []).unwrap();
    fs::write(&nonempty_file, "hello\n").unwrap();

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Empty);
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &expr,
            &EntryContext::new(empty_dir, 0, true),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &expr,
            &EntryContext::new(nonempty_dir, 0, true),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &expr,
            &EntryContext::new(empty_file, 0, true),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &expr,
            &EntryContext::new(nonempty_file, 0, true),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );
}

#[test]
fn empty_respects_follow_mode_for_root_symlinked_directories() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("empty-dir")).unwrap();
    unix_fs::symlink("empty-dir", root.path().join("empty-link")).unwrap();

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Empty);
    let entry = EntryContext::new(root.path().join("empty-link"), 0, true);
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry, FollowMode::CommandLineOnly, &mut sink).unwrap());
}

#[test]
fn current_birth_time_predicates_are_false_when_birth_time_is_unknown() {
    let Some(path) = support::existing_path_without_birth_time() else {
        return;
    };

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Newer(NewerMatcher {
        current: TimestampKind::Birth,
        reference: Timestamp::new(0, 0),
    }));
    let entry = EntryContext::new(path, 0, true);
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn current_birth_time_respects_follow_mode_for_root_symlink() {
    let root = tempdir().unwrap();
    let target = root.path().join("target.txt");
    let link = root.path().join("target-link");
    fs::write(&target, "hello\n").unwrap();
    unix_fs::symlink("target.txt", &link).unwrap();

    let Some(target_birth) = read_birth_time(&target, true).unwrap() else {
        return;
    };
    let Some(link_birth) = read_birth_time(&link, false).unwrap() else {
        return;
    };
    if target_birth == link_birth {
        return;
    }

    let (reference, physical_expected, logical_expected) = if link_birth > target_birth {
        (target_birth, true, false)
    } else {
        (link_birth, false, true)
    };
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Newer(NewerMatcher {
        current: TimestampKind::Birth,
        reference,
    }));
    let entry = EntryContext::new(link, 0, true);
    let mut sink = RecordingSink::default();

    assert_eq!(
        evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap(),
        physical_expected
    );
    assert_eq!(
        evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap(),
        logical_expected
    );
    assert_eq!(
        evaluate(&expr, &entry, FollowMode::CommandLineOnly, &mut sink).unwrap(),
        logical_expected
    );
}

#[test]
fn nonempty_directory_empty_probe_does_not_flip_used_when_atime_is_older_than_ctime() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("nonempty-dir")).unwrap();
    fs::write(root.path().join("nonempty-dir/child"), "child\n").unwrap();
    set_file_times(
        root.path(),
        Timestamp::new(1_700_000_000, 0),
        Timestamp::new(1_700_000_000, 0),
    );
    let metadata = fs::metadata(root.path()).unwrap();
    assert!(metadata.atime() < metadata.ctime());

    let expr = RuntimeExpr::or(
        RuntimeExpr::or(
            RuntimeExpr::Predicate(RuntimePredicate::Empty),
            RuntimeExpr::Predicate(RuntimePredicate::Used(UsedMatcher {
                comparison: TimeComparison::Exactly("1".parse().unwrap()),
            })),
        ),
        RuntimeExpr::Predicate(RuntimePredicate::Used(UsedMatcher {
            comparison: TimeComparison::LessThan("1".parse().unwrap()),
        })),
    );
    let entry = EntryContext::new(root.path().to_path_buf(), 0, true);
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn set_file_times(path: &Path, atime: Timestamp, mtime: Timestamp) {
    use std::ffi::CString;

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
