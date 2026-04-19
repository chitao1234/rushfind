mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::process::Command;
use support::{lines, path_arg};
use tempfile::tempdir;

#[test]
fn parallel_logical_mode_matches_gnu_find_as_a_set() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.path().join("real/b.md"), "# b\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-real")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "(".into(),
        "-name".into(),
        "*.rs".into(),
        "-o".into(),
        "-xtype".into(),
        "l".into(),
        ")".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("rfd")
        .unwrap()
        .env("RUSHFIND_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}
