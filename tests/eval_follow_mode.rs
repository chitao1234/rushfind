#![cfg(unix)]

use rushfind::ast::FileTypeFilter;
use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::output::RecordingSink;
use rushfind::planner::{RuntimeExpr, RuntimePredicate};
use std::fs;
use std::os::unix::fs as unix_fs;
use tempfile::tempdir;

#[test]
fn type_uses_the_active_follow_mode() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let entry = EntryContext::new(root.path().join("dir-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Type(FileTypeFilter::Directory));
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
}

#[test]
fn xtype_uses_the_complementary_view() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let entry = EntryContext::new(root.path().join("dir-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::XType(FileTypeFilter::Symlink));
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
}
