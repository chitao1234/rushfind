mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn ordered_exec_semicolon_false_short_circuits_later_print_but_exits_zero() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            ";".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn ordered_exec_semicolon_missing_command_is_false_and_allows_or_branch() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();
    let missing = root.path().join("definitely-missing-cmd");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            missing.as_os_str().to_os_string(),
            "{}".into(),
            ";".into(),
            "-o".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("No such file or directory")
    );
}

#[test]
fn ordered_exec_semicolon_rewrites_embedded_placeholders() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "pre{}mid{}post\\n".into(),
            ";".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("pre"));
    assert!(stdout.contains("mid"));
    assert!(stdout.contains("post"));
    assert!(stdout.contains("a.txt"));
}

#[test]
fn ordered_exec_plus_false_keeps_following_print_but_exits_one() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            "+".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
}

#[test]
fn ordered_exec_plus_false_short_circuits_or_branch_like_gnu() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            "+".into(),
            "-o".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}
