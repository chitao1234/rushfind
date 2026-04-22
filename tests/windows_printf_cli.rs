#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::escape_ls_rendered_path;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn named_ownership_printf_fields_render_non_empty_values() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(file.as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%u][%g]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout.trim_end_matches('\n').trim_end_matches('\r');
    let inner = line
        .strip_prefix('[')
        .and_then(|line| line.strip_suffix(']'));
    let parts = inner.unwrap_or("").split("][").collect::<Vec<_>>();

    assert_eq!(parts.len(), 2, "{line:?}");
    assert!(!parts[0].is_empty(), "{line:?}");
    assert!(!parts[1].is_empty(), "{line:?}");
}

#[test]
fn unix_shaped_printf_directives_are_rejected_on_windows() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    for (format, needle) in [
        ("%U\\n", "numeric ownership is not supported on Windows"),
        ("%G\\n", "numeric ownership is not supported on Windows"),
        ("%m\\n", "Unix mode bits are not supported on Windows"),
        ("%M\\n", "Unix mode bits are not supported on Windows"),
        ("%D\\n", "unsupported -printf directive on Windows"),
        ("%b\\n", "unsupported -printf directive on Windows"),
        ("%k\\n", "unsupported -printf directive on Windows"),
    ] {
        let output = cargo_bin_output_with_timeout(
            &[
                path_arg(file.as_path()),
                "-maxdepth".into(),
                "0".into(),
                "-printf".into(),
                format.into(),
            ],
            1,
            Duration::from_secs(5),
        );

        assert_eq!(output.status.code(), Some(1), "{format}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(needle), "{format}: {stderr}");
    }
}

#[test]
fn ls_renders_native_records_with_backslash_paths() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(file.as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-ls".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&escape_ls_rendered_path(&file.display().to_string())),
        "{stdout:?}"
    );
}
