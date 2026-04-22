#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::{normalize_stdout_path, symlink_creation_available};
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn flags_all_mode_matches_files_with_the_readonly_attribute() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-flags".into(),
            "-readonly".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        normalize_stdout_path(&format!("{}\n", file.display()))
    );
}

#[test]
fn reparse_type_symbolic_matches_file_symlinks() {
    if !symlink_creation_available() {
        return;
    }

    let root = tempdir().unwrap();
    let target = root.path().join("target.txt");
    let link = root.path().join("link.txt");
    fs::write(&target, b"target").unwrap();
    std::os::windows::fs::symlink_file(&target, &link).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-reparse-type".into(),
            "symbolic".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        normalize_stdout_path(&format!("{}\n", link.display()))
    );
}
