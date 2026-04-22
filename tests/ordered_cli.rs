#![cfg(unix)]

mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::process::Command;
use support::{gnu_find_output, path_arg};
use tempfile::tempdir;

#[test]
fn ordered_single_worker_matches_gnu_find_for_supported_subset() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(
        root.path().join("src/main.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();
    fs::write(root.path().join("README.md"), "# demo\n").unwrap();

    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-name".into(),
        "*.rs".into(),
    ];

    let Some(expected) = gnu_find_output(&args, false) else {
        return;
    };
    let actual = Command::cargo_bin("rfd")
        .unwrap()
        .env("RUSHFIND_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn ordered_depth_print_matches_gnu_for_supported_subset() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("README.md"), "# demo\n").unwrap();

    let args = vec![path_arg(root.path()), "-depth".into(), "-print".into()];

    let Some(expected) = gnu_find_output(&args, false) else {
        return;
    };
    let actual = Command::cargo_bin("rfd")
        .unwrap()
        .env("RUSHFIND_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}
