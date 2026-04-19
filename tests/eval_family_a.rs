use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::identity::FileIdentity;
use rushfind::numeric::NumericComparison;
use rushfind::output::RecordingSink;
use rushfind::planner::{RuntimeExpr, RuntimePredicate};
use std::fs;
use std::os::unix::fs::{self as unix_fs, MetadataExt};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn inum_uses_the_active_inode_view() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();

    let entry = entry_for(&root.path().join("file-link"), 0, true);
    let logical_inode = fs::metadata(root.path().join("file-link")).unwrap().ino();
    let physical_inode = fs::symlink_metadata(root.path().join("file-link"))
        .unwrap()
        .ino();
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Inum(NumericComparison::Exactly(
                physical_inode
            ))),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Inum(NumericComparison::Exactly(
                logical_inode
            ))),
            &entry,
            FollowMode::Logical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Inum(NumericComparison::Exactly(
                logical_inode
            ))),
            &entry_for(&root.path().join("file-link"), 1, false),
            FollowMode::CommandLineOnly,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn links_uses_the_active_hard_link_count_view() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    fs::hard_link(
        root.path().join("real.txt"),
        root.path().join("real-hard.txt"),
    )
    .unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();

    let entry = entry_for(&root.path().join("file-link"), 0, true);
    let logical_links = fs::metadata(root.path().join("file-link")).unwrap().nlink();
    let physical_links = fs::symlink_metadata(root.path().join("file-link"))
        .unwrap()
        .nlink();
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Links(NumericComparison::Exactly(
                physical_links
            ))),
            &entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::Links(NumericComparison::Exactly(
                logical_links
            ))),
            &entry,
            FollowMode::Logical,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn samefile_uses_active_identity_and_root_only_h_semantics() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();

    let root_entry = entry_for(&root.path().join("file-link"), 0, true);
    let nested_entry = entry_for(&root.path().join("file-link"), 1, false);
    let physical_identity =
        FileIdentity::from_metadata(&fs::symlink_metadata(root.path().join("file-link")).unwrap());
    let logical_identity =
        FileIdentity::from_metadata(&fs::metadata(root.path().join("file-link")).unwrap());
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::SameFile(physical_identity)),
            &nested_entry,
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::SameFile(logical_identity)),
            &root_entry,
            FollowMode::CommandLineOnly,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &RuntimeExpr::Predicate(RuntimePredicate::SameFile(logical_identity)),
            &nested_entry,
            FollowMode::CommandLineOnly,
            &mut sink,
        )
        .unwrap()
    );
}

fn entry_for(path: &Path, depth: usize, is_command_line_root: bool) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, is_command_line_root)
}
