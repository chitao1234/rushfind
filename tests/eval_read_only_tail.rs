use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
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

    assert!(evaluate(
        &expr,
        &EntryContext::new(empty_dir, 0, true),
        FollowMode::Physical,
        &mut sink
    )
    .unwrap());
    assert!(!evaluate(
        &expr,
        &EntryContext::new(nonempty_dir, 0, true),
        FollowMode::Physical,
        &mut sink
    )
    .unwrap());
    assert!(evaluate(
        &expr,
        &EntryContext::new(empty_file, 0, true),
        FollowMode::Physical,
        &mut sink
    )
    .unwrap());
    assert!(!evaluate(
        &expr,
        &EntryContext::new(nonempty_file, 0, true),
        FollowMode::Physical,
        &mut sink
    )
    .unwrap());
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
