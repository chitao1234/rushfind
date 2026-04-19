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

#[cfg(unix)]
#[test]
fn rust_mode_accepts_non_utf8_literal_bytes_in_patterns() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempdir().unwrap();
    let file_name = OsString::from_vec(vec![b'f', b'o', b'o', 0xff]);
    let path = root.path().join(PathBuf::from(file_name));
    fs::write(&path, "demo\n").unwrap();
    let pattern = OsString::from_vec(vec![b'.', b'*', b'/', b'f', b'o', b'o', 0xff]);
    let matcher =
        RegexMatcher::compile("-regex", RegexDialect::Rust, pattern.as_os_str(), false).unwrap();

    assert!(matcher.is_match(path.as_os_str()).unwrap());
}

#[cfg(unix)]
#[test]
fn gnu_facing_dialects_accept_non_utf8_literal_bytes_in_patterns() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempdir().unwrap();
    let file_name = OsString::from_vec(vec![b'b', b'a', b'r', 0xfe]);
    let path = root.path().join(PathBuf::from(file_name));
    fs::write(&path, "demo\n").unwrap();
    let pattern = OsString::from_vec(vec![b'.', b'*', b'/', b'b', b'a', b'r', 0xfe]);
    let matcher = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixExtended,
        pattern.as_os_str(),
        false,
    )
    .unwrap();

    assert!(matcher.is_match(path.as_os_str()).unwrap());
}

#[test]
fn posix_basic_supports_bre_escaped_grouping_alternation_and_repetition() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    let lib_path = root.path().join("src/lib.rs");
    let main_path = root.path().join("src/main.rs");
    fs::write(&lib_path, "pub fn lib() {}\n").unwrap();
    fs::write(&main_path, "fn main() {}\n").unwrap();

    let matcher = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/src/\(lib\|main\)\.rs"),
        false,
    )
    .unwrap();
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(matcher));
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &expr,
            &entry_for(&lib_path, 1),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );
    assert!(
        evaluate(
            &expr,
            &entry_for(&main_path, 1),
            FollowMode::Physical,
            &mut sink
        )
        .unwrap()
    );

    let bounded = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/src/[[:alpha:]]\{3\}\.rs"),
        false,
    )
    .unwrap();
    let bounded_expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(bounded));

    assert!(
        evaluate(
            &bounded_expr,
            &entry_for(&lib_path, 1),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &bounded_expr,
            &entry_for(&main_path, 1),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn gnu_facing_named_classes_use_ascii_c_locale_semantics() {
    let ascii = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(r".*/[[:alpha:]][[:digit:]]\.txt"),
        false,
    )
    .unwrap();

    assert!(ascii.is_match(OsStr::new("./A7.txt")).unwrap());
    assert!(!ascii.is_match(OsStr::new("./é7.txt")).unwrap());
}

#[test]
fn pcre2_regextype_remains_whole_path_anchored() {
    let matcher = RegexMatcher::compile("-regex", RegexDialect::Pcre2, OsStr::new("lib"), false)
        .unwrap();

    assert!(!matcher.is_match(OsStr::new("./src/lib.rs")).unwrap());
}

#[test]
fn gnu_foundation_posix_extended_unmatched_close_paren_matches_literal_names() {
    let matcher = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(".*/paren)"),
        false,
    )
    .unwrap();

    assert!(matcher.is_match(OsStr::new("./paren)")).unwrap());
}

#[test]
fn gnu_foundation_posix_basic_contextual_plus_and_question_match_gnu_examples() {
    let plus = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/\(\+foo\)"),
        false,
    )
    .unwrap();
    let question = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/\(\?foo\)"),
        false,
    )
    .unwrap();

    assert!(plus.is_match(OsStr::new("./+foo")).unwrap());
    assert!(question.is_match(OsStr::new("./?foo")).unwrap());
}

#[test]
fn gnu_foundation_bre_and_ere_backreferences_match() {
    let bre = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/\(.\)\1"),
        false,
    )
    .unwrap();
    let ere = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(r".*/(.)\1"),
        false,
    )
    .unwrap();

    assert!(bre.is_match(OsStr::new("./aa")).unwrap());
    assert!(ere.is_match(OsStr::new("./aa")).unwrap());
}

#[test]
fn emacs_followup_backreferences_match_through_eval() {
    let root = tempdir().unwrap();
    let repeated = root.path().join("aa");
    let non_repeated = root.path().join("ab");
    fs::write(&repeated, "aa\n").unwrap();
    fs::write(&non_repeated, "ab\n").unwrap();

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile(
            "-regex",
            RegexDialect::Emacs,
            OsStr::new(r".*/\(.\)\1"),
            false,
        )
        .unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(
        evaluate(
            &expr,
            &entry_for(&repeated, 0),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
    assert!(
        !evaluate(
            &expr,
            &entry_for(&non_repeated, 0),
            FollowMode::Physical,
            &mut sink,
        )
        .unwrap()
    );
}

#[test]
fn emacs_followup_mixed_alternation_and_backreference_match_through_eval() {
    let root = tempdir().unwrap();
    let abab = root.path().join("abab");
    let cdcd = root.path().join("cdcd");
    let abcd = root.path().join("abcd");
    fs::write(&abab, "abab\n").unwrap();
    fs::write(&cdcd, "cdcd\n").unwrap();
    fs::write(&abcd, "abcd\n").unwrap();

    let expr = RuntimeExpr::Predicate(RuntimePredicate::Regex(
        RegexMatcher::compile(
            "-regex",
            RegexDialect::Emacs,
            OsStr::new(r".*/\(ab\|cd\)\1"),
            false,
        )
        .unwrap(),
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry_for(&abab, 0), FollowMode::Physical, &mut sink).unwrap());
    assert!(evaluate(&expr, &entry_for(&cdcd, 0), FollowMode::Physical, &mut sink).unwrap());
    assert!(!evaluate(&expr, &entry_for(&abcd, 0), FollowMode::Physical, &mut sink).unwrap());
}

#[test]
fn gnu_foundation_bre_and_ere_gnu_escapes_match() {
    let word = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(r".*/\<foo\>"),
        false,
    )
    .unwrap();
    let boundary = RegexMatcher::compile(
        "-regex",
        RegexDialect::PosixBasic,
        OsStr::new(r".*/\bfoo\b"),
        false,
    )
    .unwrap();

    assert!(word.is_match(OsStr::new("./foo")).unwrap());
    assert!(boundary.is_match(OsStr::new("./foo")).unwrap());
    assert!(!word.is_match(OsStr::new("./foobar")).unwrap());
}

fn entry_for(path: &Path, depth: usize) -> EntryContext {
    EntryContext::new(PathBuf::from(path), depth, true)
}
