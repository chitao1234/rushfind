mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

fn expected_version_stdout() -> String {
    format!(
        "rushfind {} (commit {}, target {})\n",
        env!("RUSHFIND_BUILD_VERSION"),
        env!("RUSHFIND_BUILD_GIT_HASH"),
        env!("RUSHFIND_BUILD_TARGET"),
    )
}

#[test]
fn version_aliases_print_build_metadata_and_exit_successfully() {
    for raw in ["-version", "--version"] {
        let output = cargo_bin_output_with_timeout(&[raw.into()], 1, Duration::from_secs(5));

        assert_eq!(output.status.code(), Some(0), "{raw}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected_version_stdout(),
            "{raw}"
        );
        assert!(
            output.stderr.is_empty(),
            "{raw}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn version_short_circuits_traversal_and_actions() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            "--version".into(),
            path_arg(file.as_path()),
            "-delete".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        expected_version_stdout()
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        file.exists(),
        "version flags must bypass traversal and -delete"
    );
}

#[test]
fn help_prints_human_first_quick_reference() {
    let output = cargo_bin_output_with_timeout(&["--help".into()], 1, Duration::from_secs(5));

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "Usage: rfd [global options] [path ...] [expression]",
        "path defaults to . and expression defaults to -print",
        "Global options:",
        "-P -H -L",
        "-Olevel",
        "-D opts",
        "Compatibility options:",
        "-files0-from FILE|-",
        "-follow",
        "Common tests:",
        "-context LABEL",
        "-type/-xtype LIST",
        "-size N",
        "Actions:",
        "-printf FORMAT",
        "-exec ... ;",
        "-delete",
        "Environment:",
        "RUSHFIND_WORKERS",
        "See rfd(1) for the full reference.",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }

    assert!(
        !stdout.contains("parser subset"),
        "help must not expose implementation wording:\n{stdout}"
    );
}

#[test]
fn help_short_circuits_traversal_and_actions() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &["--help".into(), path_arg(file.as_path()), "-delete".into()],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("Usage: rfd [global options] [path ...] [expression]\n"),
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(file.exists(), "help must bypass traversal and -delete");
}

#[test]
fn debug_help_explains_lightweight_diagnostics() {
    let output =
        cargo_bin_output_with_timeout(&["-D".into(), "help".into()], 1, Duration::from_secs(5));

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "Debug categories accepted by rfd -D:",
        "exec   opt   rates   search   stat   time   tree   all   help",
        "lightweight rushfind diagnostics",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }
}
