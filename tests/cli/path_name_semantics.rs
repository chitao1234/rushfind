#![cfg(unix)]

//! GNU name semantics for `%f`, `%h`, `-name`/`-iname` and `-execdir`'s `{}`.
//!
//! GNU find derives these from gnulib's `base_name()` (plus
//! `strip_trailing_slashes()` for the name predicates) and its own leading
//! directories rule for `%h`.  Rust's `Path::file_name()`/`Path::parent()`
//! instead report `None`/`""` for degenerate spellings such as ".", "/", "a/"
//! and "a/b/..", so the call sites must not use them.  `%p`, `%P` and `%H` keep
//! echoing the path as given, whatever that looks like.

use crate::support::{argv, ensure_gnu_find, gnu_find_command, rushfind_command_with_workers};
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// `(start path, %f, %h, -execdir placeholder, basenames -name matches)`.
///
/// Every start path exists under the fixture tree, so the matrix also covers
/// spellings that keep a trailing slash or repeat separators.
const MATRIX: &[(&str, &str, &str, &str, &[&str])] = &[
    (".", ".", ".", "./.", &["."]),
    ("..", "..", ".", "./..", &[".."]),
    ("/", "/", "", "/", &["/"]),
    ("a", "a", ".", "./a", &["a"]),
    ("a/", "a/", "a", "./a/", &["a"]),
    ("./a", "a", ".", "./a", &["a"]),
    ("a/b", "b", "a", "./b", &["b"]),
    ("a/b/", "b/", "a", "./b/", &["b"]),
    ("a/..", "..", "a", "./..", &[".."]),
    ("a/b/..", "..", "a/b", "./..", &[".."]),
    ("./", "./", ".", "././", &["."]),
    ("a/b/c", "c", "a/b", "./c", &["c"]),
    ("./a/b/c/", "c/", "./a/b", "./c/", &["c"]),
    (".//", "./", "./", "././", &["."]),
    ("./a//", "a/", ".", "./a/", &["a"]),
    ("a//b//", "b/", "a/", "./b/", &["b"]),
    ("a/./b", "b", "a/.", "./b", &["b"]),
    ("./a/./b", "b", "./a/.", "./b", &["b"]),
    ("//", "/", "/", "/", &["/"]),
    ("///", "/", "//", "/", &["/"]),
    ("////", "/", "///", "/", &["/"]),
];

/// Enough spellings to tell the gnulib basename apart from the raw path tail.
const NAME_CANDIDATES: &[&str] = &[".", "..", "/", "a", "b", "c", "a/", "b/"];

fn fixture_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    let leaf = root.path().join("a").join("b").join("c");
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("f.txt"), b"").unwrap();
    root
}

fn run_in(dir: &Path, args: &[OsString]) -> Vec<u8> {
    let output = rushfind_command_with_workers(1)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "rfd {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn gnu_in(dir: &Path, args: &[OsString]) -> Option<Vec<u8>> {
    if !ensure_gnu_find() {
        return None;
    }

    let output = gnu_find_command()
        .unwrap()
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "gfind {args:?} failed");
    Some(output.stdout)
}

fn printf_args(start: &str) -> Vec<OsString> {
    argv(&[start, "-maxdepth", "0", "-printf", "[%f][%h]\\n"])
}

fn execdir_args(start: &str) -> Vec<OsString> {
    argv(&[start, "-maxdepth", "0", "-execdir", "echo", "{}", ";"])
}

/// `-name <candidate> -printf <candidate>` chained with `-o` for every
/// candidate, so one run reports exactly the candidates that match.
fn name_probe_args(start: &str) -> Vec<OsString> {
    let mut args = vec![start.into(), "-maxdepth".into(), "0".into()];
    for (index, candidate) in NAME_CANDIDATES.iter().enumerate() {
        if index > 0 {
            args.push("-o".into());
        }
        args.push("-name".into());
        args.push((*candidate).into());
        args.push("-printf".into());
        args.push(format!("{candidate}\\n").into());
    }

    args
}

/// Start paths that exercise the root-adjacent and absolute spellings, as
/// `(path, %f, %h)`.
fn absolute_paths(root: &Path) -> Vec<(String, String, String)> {
    let absolute = root.to_str().unwrap().to_string();
    let parent = root.parent().unwrap().to_str().unwrap();
    let name = root.file_name().unwrap().to_str().unwrap();

    let mut paths = vec![
        (absolute.clone(), name.to_string(), parent.to_string()),
        (
            format!("{absolute}/"),
            format!("{name}/"),
            parent.to_string(),
        ),
        (
            format!("{absolute}/a/b/c"),
            "c".to_string(),
            format!("{absolute}/a/b"),
        ),
        (
            format!("{absolute}/a/b/c/"),
            "c/".to_string(),
            format!("{absolute}/a/b"),
        ),
        (
            format!("{absolute}//a///b//"),
            "b/".to_string(),
            format!("{absolute}//a//"),
        ),
    ];

    // A path one level below the root is where %h turns empty.
    if Path::new("/tmp").is_dir() {
        paths.push(("/tmp".to_string(), "tmp".to_string(), String::new()));
        paths.push(("/tmp/".to_string(), "tmp/".to_string(), String::new()));
    }

    paths
}

#[test]
fn printf_f_and_h_use_the_gnulib_names() {
    let root = fixture_tree();

    for (start, basename, dirname, _, _) in MATRIX {
        assert_eq!(
            run_in(root.path(), &printf_args(start)),
            format!("[{basename}][{dirname}]\n").into_bytes(),
            "start path {start:?}"
        );
    }
}

#[test]
fn name_predicates_use_the_gnulib_basename() {
    let root = fixture_tree();

    for (start, _, _, _, matches) in MATRIX {
        let mut expected = matches.join("\n");
        if !expected.is_empty() {
            expected.push('\n');
        }

        assert_eq!(
            run_in(root.path(), &name_probe_args(start)),
            expected.into_bytes(),
            "start path {start:?}"
        );
    }
}

#[test]
fn execdir_uses_the_gnulib_basename() {
    let root = fixture_tree();

    for (start, _, _, placeholder, _) in MATRIX {
        assert_eq!(
            run_in(root.path(), &execdir_args(start)),
            format!("{placeholder}\n").into_bytes(),
            "start path {start:?}"
        );
    }
}

#[test]
fn absolute_start_paths_use_the_gnulib_names() {
    let root = fixture_tree();

    for (start, basename, dirname) in absolute_paths(root.path()) {
        assert_eq!(
            run_in(root.path(), &printf_args(&start)),
            format!("[{basename}][{dirname}]\n").into_bytes(),
            "start path {start:?}"
        );
    }
}

#[test]
fn children_of_a_slashed_start_path_keep_their_own_spelling() {
    let root = fixture_tree();

    for (start, child, child_dirname) in [
        ("a/", "a/b", "a"),
        ("a//", "a//b", "a/"),
        ("./a/", "./a/b", "./a"),
        ("./a//", "./a//b", "./a/"),
    ] {
        let output = run_in(
            root.path(),
            &argv(&[
                start,
                "-mindepth",
                "1",
                "-maxdepth",
                "1",
                "-printf",
                "[%p][%f][%h]\\n",
            ]),
        );

        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("[{child}][b][{child_dirname}]\n"),
            "start path {start:?}"
        );
    }
}

#[test]
fn name_semantics_match_gnu_find() {
    let root = fixture_tree();
    let starts = MATRIX.iter().map(|(start, ..)| (*start).to_string()).chain(
        absolute_paths(root.path())
            .into_iter()
            .map(|(start, ..)| start),
    );

    for start in starts {
        for args in [
            printf_args(&start),
            name_probe_args(&start),
            execdir_args(&start),
        ] {
            let Some(expected) = gnu_in(root.path(), &args) else {
                return;
            };

            assert_eq!(
                run_in(root.path(), &args),
                expected,
                "start path {start:?} args {args:?}"
            );
        }
    }
}
