#![cfg(unix)]

use crate::support::{gnu_find_output, path_arg, rushfind_command};
use std::fs;
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
    let actual = rushfind_command()
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
    let actual = rushfind_command()
        .env("RUSHFIND_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn sorted_traversal_orders_each_directory_by_path_bytes() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("zdir")).unwrap();
    fs::create_dir(root.path().join("adir")).unwrap();
    fs::write(root.path().join("b"), "b\n").unwrap();
    fs::write(root.path().join("a"), "a\n").unwrap();
    fs::write(root.path().join("zdir/z"), "z\n").unwrap();
    fs::write(root.path().join("adir/a"), "a\n").unwrap();

    let args = vec!["-s".into(), path_arg(root.path()), "-print".into()];
    let actual = rushfind_command()
        .env("RUSHFIND_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    let expected = [
        root.path().display().to_string(),
        root.path().join("a").display().to_string(),
        root.path().join("adir").display().to_string(),
        root.path().join("adir/a").display().to_string(),
        root.path().join("b").display().to_string(),
        root.path().join("zdir").display().to_string(),
        root.path().join("zdir/z").display().to_string(),
    ]
    .join("\n")
        + "\n";
    assert_eq!(actual.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&actual.stdout), expected);
}

#[test]
fn sorted_depth_traversal_orders_each_directory_before_parent() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("zdir")).unwrap();
    fs::create_dir(root.path().join("adir")).unwrap();
    fs::write(root.path().join("zdir/z"), "z\n").unwrap();
    fs::write(root.path().join("adir/a"), "a\n").unwrap();

    let args = vec![
        "-d".into(),
        "-s".into(),
        path_arg(root.path()),
        "-print".into(),
    ];
    let actual = rushfind_command()
        .env("RUSHFIND_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    let expected = [
        root.path().join("adir/a").display().to_string(),
        root.path().join("adir").display().to_string(),
        root.path().join("zdir/z").display().to_string(),
        root.path().join("zdir").display().to_string(),
        root.path().display().to_string(),
    ]
    .join("\n")
        + "\n";
    assert_eq!(actual.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&actual.stdout), expected);
}
