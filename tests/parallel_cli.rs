mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::process::Command;
use support::{lines, path_arg};
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

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}
