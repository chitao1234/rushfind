use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::perm::parse_perm_argument;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn perm_exact_matches_exact_mode_only() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    fs::set_permissions(root.path().join("file.txt"), fs::Permissions::from_mode(0o664)).unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("664")).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn perm_all_bits_and_any_bits_match_symbolic_forms() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    fs::set_permissions(root.path().join("file.txt"), fs::Permissions::from_mode(0o660)).unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let mut sink = RecordingSink::default();

    let all_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("-g+w,u+w")).unwrap(),
    ));
    let any_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("/u=w,g=w")).unwrap(),
    ));
    let copy_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("-g=u")).unwrap(),
    ));

    assert!(evaluate(&all_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&any_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&copy_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn perm_symbolic_zero_baseline_matches_gnu_find_behavior() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    fs::set_permissions(root.path().join("file.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let mut sink = RecordingSink::default();

    let exact_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("g=u")).unwrap(),
    ));
    let all_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("-g=u")).unwrap(),
    ));
    let any_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("/g=u")).unwrap(),
    ));
    let x_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("-u=X")).unwrap(),
    ));
    let empty_assign_expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("u=")).unwrap(),
    ));

    assert!(evaluate(&exact_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&all_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&any_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&x_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&empty_assign_expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn perm_exact_symbolic_sticky_matches() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    fs::set_permissions(
        root.path().join("file.txt"),
        fs::Permissions::from_mode(0o1000),
    )
    .unwrap();
    let entry = entry_for(&root.path().join("file.txt"));
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Perm(
        parse_perm_argument(OsStr::new("+t")).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn entry_for(path: &Path) -> EntryContext {
    EntryContext::new(
        PathBuf::from(path),
        0,
        true,
        fs::symlink_metadata(path).unwrap(),
        fs::metadata(path).ok(),
    )
}
