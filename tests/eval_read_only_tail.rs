use findoxide::birth::read_birth_time;
use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
use findoxide::time::{NewerMatcher, Timestamp, TimestampKind};
use std::fs;
use std::os::unix::fs as unix_fs;
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
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Newer(NewerMatcher {
        current: TimestampKind::Birth,
        reference: Timestamp::new(0, 0),
    }));
    let entry = EntryContext::new("/proc/self/stat".into(), 0, true);
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
