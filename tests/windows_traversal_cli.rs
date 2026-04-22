#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::{normalize_stdout_path, symlink_creation_available};
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn logical_follow_on_directory_symlink_keeps_windows_path_spelling() {
    if !symlink_creation_available() {
        return;
    }

    let root = tempdir().unwrap();
    let real = root.path().join("real");
    let link = root.path().join("link");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("alpha.txt"), b"alpha").unwrap();
    std::os::windows::fs::symlink_dir(&real, &link).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            "-L".into(),
            path_arg(link.as_path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        normalize_stdout_path(&format!("{}\n", link.join("alpha.txt").display()))
    );
}
