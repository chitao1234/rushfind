use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::numeric::NumericComparison;
use rushfind::output::RecordingSink;
use rushfind::planner::{RuntimeExpr, RuntimePredicate};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn uid_and_gid_match_active_numeric_metadata() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let metadata = fs::metadata(root.path().join("file.txt")).unwrap();
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Uid(NumericComparison::Exactly(
                metadata.uid().into()
            ))),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Gid(NumericComparison::Exactly(
                metadata.gid().into()
            ))),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn user_and_group_match_exact_ids() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let metadata = fs::metadata(root.path().join("file.txt")).unwrap();
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::User(metadata.uid())),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Group(metadata.gid())),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn nouser_and_nogroup_are_false_for_current_tempfile_ids() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let mut sink = RecordingSink::default();

    assert!(
        !evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::NoUser),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );

    assert!(
        !evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::NoGroup),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

fn entry_for(path: &Path) -> EntryContext {
    EntryContext::new(PathBuf::from(path), 0, true)
}
