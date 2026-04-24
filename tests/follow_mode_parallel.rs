#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs as unix_fs;
use support::{gnu_find_output, lines, path_arg, rushfind_command};
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
