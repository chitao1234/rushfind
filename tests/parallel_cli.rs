#![cfg(unix)]

mod support;

use std::fs;
use support::{gnu_find_output, lines, path_arg, rushfind_command};
use tempfile::tempdir;

#[test]
fn parallel_mode_matches_gnu_find_as_an_unordered_set() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.path().join("tests/cli.rs"), "#[test]\nfn smoke() {}\n").unwrap();
    fs::write(root.path().join("README.md"), "# demo\n").unwrap();

    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "(".into(),
        "-name".into(),
        "*.rs".into(),
        "-o".into(),
        "-name".into(),
        "*.md".into(),
        ")".into(),
    ];

    let Some(expected) = gnu_find_output(&args, false) else {
        return;
    };
    let actual = rushfind_command()
        .env("RUSHFIND_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}
