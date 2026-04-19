use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::output::RecordingSink;
use rushfind::planner::{RuntimeExpr, RuntimePredicate};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn lname_matches_physical_link_contents_under_p() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();

    let entry = entry_for(&root.path().join("file-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*real.txt".into(),
        case_insensitive: false,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn lname_returns_false_for_resolved_symlinks_under_l() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();

    let entry = entry_for(&root.path().join("file-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*real.txt".into(),
        case_insensitive: false,
    });
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
}

#[test]
fn broken_symlink_still_matches_under_l() {
    let root = tempdir().unwrap();
    unix_fs::symlink("missing-target", root.path().join("broken-link")).unwrap();

    let entry = entry_for(&root.path().join("broken-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*missing*".into(),
        case_insensitive: false,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Logical, &mut sink).unwrap());
}

#[test]
fn h_root_symlink_is_logical_but_non_root_symlink_is_physical() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("root-link")).unwrap();
    unix_fs::symlink(
        root.path().join("real/file.txt"),
        root.path().join("child-link"),
    )
    .unwrap();

    let root_expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*real".into(),
        case_insensitive: false,
    });
    let child_expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*file.txt".into(),
        case_insensitive: false,
    });
    let mut sink = RecordingSink::default();

    assert!(
        !evaluate(
            &root_expr,
            &entry_for(&root.path().join("root-link"), 0, true),
            FollowMode::CommandLineOnly,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &child_expr,
            &entry_for(&root.path().join("child-link"), 1, false),
            FollowMode::CommandLineOnly,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn broken_h_root_symlink_still_matches_by_link_contents() {
    let root = tempdir().unwrap();
    unix_fs::symlink("missing-target", root.path().join("broken-root")).unwrap();

    let entry = entry_for(&root.path().join("broken-root"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*missing*".into(),
        case_insensitive: false,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::CommandLineOnly, &mut sink).unwrap());
}

#[test]
fn ilname_is_case_insensitive() {
    let root = tempdir().unwrap();
    unix_fs::symlink("MiXeD-TaRgEt", root.path().join("mixed-link")).unwrap();

    let entry = entry_for(&root.path().join("mixed-link"), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::LName {
        pattern: "*mixed-target".into(),
        case_insensitive: true,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn entry_for(path: &Path, depth: usize, is_command_line_root: bool) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, is_command_line_root)
}
