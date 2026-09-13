mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, newline_records, path_arg};
use tempfile::tempdir;

#[test]
fn exit_stops_after_the_current_entry_and_returns_status() {
    let root = tempdir().unwrap();
    let first = root.path().join("a");
    let second = root.path().join("b");
    fs::write(&first, b"a").unwrap();
    fs::write(&second, b"b").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-print".into(),
            "-exit".into(),
            "17".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(17));
    assert_eq!(newline_records(&output.stdout).len(), 1);
}

#[test]
fn exit_without_status_returns_zero() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a"), b"a").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exit".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}
