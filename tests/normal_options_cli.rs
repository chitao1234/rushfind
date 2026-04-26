mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn help_short_circuits_traversal_and_actions() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &["--help".into(), path_arg(&file), "-delete".into()],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage:"), "{stdout}");
    assert!(stdout.contains("Compatibility options:"), "{stdout}");
    assert!(stdout.contains("Common tests:"), "{stdout}");
    assert!(stdout.contains("Actions:"), "{stdout}");
    assert!(stdout.contains("Environment:"), "{stdout}");
    assert!(stdout.contains("-files0-from"), "{stdout}");
    assert!(stdout.contains("RUSHFIND_WORKERS"), "{stdout}");
    assert!(stdout.contains("See rfd(1)"), "{stdout}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(file.exists(), "help must bypass traversal and -delete");
}

#[test]
fn debug_help_short_circuits_with_internal_debug_help() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            "-D".into(),
            "help".into(),
            path_arg(root.path()),
            "-delete".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Debug categories"), "{stdout}");
    assert!(stdout.contains("search"), "{stdout}");
    assert!(
        stdout.contains("lightweight rushfind diagnostics"),
        "{stdout}"
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        file.exists(),
        "debug help must bypass traversal and -delete"
    );
}

#[test]
fn known_debug_options_emit_lightweight_diagnostics_and_continue() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            "-D".into(),
            "search".into(),
            path_arg(root.path()),
            "-maxdepth".into(),
            "0".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("debug[search]"), "{stderr}");
    assert!(stderr.contains("not implemented"), "{stderr}");
}

#[test]
fn warning_mode_controls_unknown_debug_warnings() {
    let root = tempdir().unwrap();

    let warned = cargo_bin_output_with_timeout(
        &[
            "-D".into(),
            "unknown".into(),
            path_arg(root.path()),
            "-maxdepth".into(),
            "0".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(warned.status.code(), Some(0));
    assert!(
        String::from_utf8(warned.stderr)
            .unwrap()
            .contains("unknown debug option")
    );

    let suppressed = cargo_bin_output_with_timeout(
        &[
            "-D".into(),
            "unknown".into(),
            path_arg(root.path()),
            "-nowarn".into(),
            "-maxdepth".into(),
            "0".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(suppressed.status.code(), Some(0));
    assert!(
        !String::from_utf8(suppressed.stderr)
            .unwrap()
            .contains("unknown debug option")
    );

    let reenabled = cargo_bin_output_with_timeout(
        &[
            "-D".into(),
            "unknown".into(),
            path_arg(root.path()),
            "-nowarn".into(),
            "-warn".into(),
            "-maxdepth".into(),
            "0".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(reenabled.status.code(), Some(0));
    assert!(
        String::from_utf8(reenabled.stderr)
            .unwrap()
            .contains("unknown debug option")
    );
}
