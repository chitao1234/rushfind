#![cfg(unix)]

mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::process::Command;
use support::{gnu_find_output, path_arg};
use tempfile::tempdir;

#[test]
fn logical_mode_descends_through_symlinked_directories() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-real")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "file.txt".into(),
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
fn command_line_only_mode_follows_symlinked_start_paths() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("root-link")).unwrap();

    let args = vec![
        "-H".into(),
        path_arg(&root.path().join("root-link")),
        "-name".into(),
        "file.txt".into(),
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
