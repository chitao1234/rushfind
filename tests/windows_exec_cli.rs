#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_input_timeout, cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn execdir_uses_dot_backslash_placeholder() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/alpha.txt"), b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "alpha.txt".into(),
            "-execdir".into(),
            "cmd".into(),
            "/C".into(),
            "echo".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout
            .lines()
            .any(|line| line.trim_end_matches('\r') == ".\\alpha.txt"),
        "{stdout:?}"
    );
}

#[test]
fn okdir_prompt_renders_dot_backslash_placeholder() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/alpha.txt"), b"alpha").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "alpha.txt".into(),
            "-okdir".into(),
            "cmd".into(),
            "/C".into(),
            "echo".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        b"n\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(".\\alpha.txt"), "{stderr:?}");
}
