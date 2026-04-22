use rushfind::entry::EntryContext;
use rushfind::eval::evaluate;
use rushfind::follow::FollowMode;
use rushfind::output::RecordingSink;
use rushfind::pattern::{CompiledGlob, GlobCaseMode, GlobSlashMode};
use rushfind::planner::{OutputAction, RuntimeAction, RuntimeExpr, RuntimePredicate};
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
    let expr = RuntimeExpr::and(vec![
        RuntimeExpr::Predicate(RuntimePredicate::Name(
            CompiledGlob::compile(
                "-name",
                std::ffi::OsStr::new("*.rs"),
                GlobCaseMode::Sensitive,
                GlobSlashMode::Literal,
            )
            .unwrap(),
        )),
        RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
    ]);
    let mut sink = RecordingSink::default();

    let matched = evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap();

    assert!(matched);
    assert_eq!(sink.into_utf8(), format!("{}\n", normalized_display(&path)));
}

#[test]
fn iname_predicate_is_case_insensitive() {
    let root = tempdir().unwrap();
    let path = root.path().join("README.MD");
    fs::write(&path, "# demo\n").unwrap();
    let entry = entry_for(&path, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Name(
        CompiledGlob::compile(
            "-iname",
            std::ffi::OsStr::new("*.md"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn path_predicate_matches_across_slashes_like_gnu_find() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("src/nested")).unwrap();
    let path = root.path().join("src/nested/lib.rs");
    fs::write(&path, "pub fn lib() {}\n").unwrap();
    let pattern = format!("{}/src/*", root.path().display()).replace('\\', "/");
    let entry = entry_for(&path, 2);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Path(
        CompiledGlob::compile(
            "-path",
            std::ffi::OsStr::new(&pattern),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap(),
    ));
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
        rushfind::ast::FileTypeFilter::Directory,
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn entry_for(path: &Path, depth: usize) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, true)
}

fn normalized_display(path: &Path) -> String {
    #[cfg(windows)]
    {
        path.display().to_string().replace('/', "\\")
    }

    #[cfg(unix)]
    {
        path.display().to_string()
    }
}
