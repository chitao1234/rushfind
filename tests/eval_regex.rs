use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
use findoxide::regex_match::{RegexDialect, RegexMatcher};
use std::ffi::OsStr;
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn regex_matches_the_whole_path_not_a_substring() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let path = root.path().join("src/lib.rs");
    fs::write(&path, "pub fn lib() {}\n").unwrap();
    let entry = entry_for(&path, 1);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile("-regex", RegexDialect::Rust, OsStr::new("lib"), false).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(!evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn iregex_uses_case_insensitive_matching() {
    let root = tempdir().unwrap();
    let path = root.path().join("README.MD");
    fs::write(&path, "# demo\n").unwrap();
    let entry = entry_for(&path, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile(
            "-iregex",
            RegexDialect::Rust,
            OsStr::new(".*readme\\.md"),
            true,
        )
        .unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn rust_mode_accepts_rust_specific_grouping_syntax() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let path = root.path().join("src/lib.rs");
    fs::write(&path, "pub fn lib() {}\n").unwrap();
    let entry = entry_for(&path, 1);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile(
            "-regex",
            RegexDialect::Rust,
            OsStr::new(".*/(?:src|docs)/.*\\.rs"),
            false,
        )
        .unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

#[cfg(unix)]
#[test]
fn non_utf8_candidate_paths_are_matched_without_lossy_conversion() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempdir().unwrap();
    let file_name = OsString::from_vec(vec![b'b', b'i', b'n', 0xff]);
    let path = root.path().join(PathBuf::from(file_name));
    fs::write(&path, "binary\n").unwrap();
    let entry = entry_for(&path, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile("-regex", RegexDialect::Rust, OsStr::new(".*bin."), false).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
}

fn entry_for(path: &Path, depth: usize) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, true)
}
