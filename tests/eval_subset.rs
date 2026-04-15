use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::planner::{OutputAction, RuntimeExpr, RuntimePredicate};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn matching_name_predicate_prints_the_entry_path() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let path = root.path().join("src/lib.rs");
    fs::write(&path, "pub fn lib() {}\n").unwrap();
    let entry = entry_for(&path, 1);
    let expr = RuntimeExpr::And(vec![
        RuntimeExpr::Predicate(RuntimePredicate::Name {
            pattern: "*.rs".into(),
            case_insensitive: false,
        }),
        RuntimeExpr::Action(OutputAction::Print),
    ]);
    let mut sink = RecordingSink::default();

    let matched = evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap();

    assert!(matched);
    assert_eq!(sink.into_utf8(), format!("{}\n", path.display()));
}

#[test]
fn iname_predicate_is_case_insensitive() {
    let root = tempdir().unwrap();
    let path = root.path().join("README.MD");
    fs::write(&path, "# demo\n").unwrap();
    let entry = entry_for(&path, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Name {
        pattern: "*.md".into(),
        case_insensitive: true,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn type_predicate_filters_by_entry_kind() {
    let root = tempdir().unwrap();
    let path = root.path().join("src");
    fs::create_dir(&path).unwrap();
    let entry = entry_for(&path, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Type(
        findoxide::ast::FileTypeFilter::Directory,
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn entry_for(path: &Path, depth: usize) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, true)
}
