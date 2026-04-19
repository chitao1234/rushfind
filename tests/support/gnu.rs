use super::lines;
use assert_cmd::cargo::CommandCargoExt;
use std::ffi::OsString;
use std::fs;
use std::process::{Command, Output};
use tempfile::tempdir;

pub const PRINTF_TIME_TZ: &str = "Asia/Shanghai";

fn apply_common_env(command: &mut Command) {
    command.env("LC_ALL", "C").env("TZ", PRINTF_TIME_TZ);
}

fn gnu_find_output(args: &[OsString], with_env: bool) -> Output {
    let mut command = Command::new("find");
    if with_env {
        apply_common_env(&mut command);
    }
    command.args(args).output().unwrap()
}

fn findoxide_output(args: &[OsString], workers: usize, with_env: bool) -> Output {
    let mut command = Command::cargo_bin("findoxide").unwrap();
    command.env("FINDOXIDE_WORKERS", workers.to_string());
    if with_env {
        apply_common_env(&mut command);
    }
    command.args(args).output().unwrap()
}

pub fn assert_matches_gnu_exact(args: &[OsString]) {
    let expected = gnu_find_output(args, false);
    let actual = findoxide_output(args, 1, false);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

pub fn assert_matches_gnu_as_sets(args: &[OsString]) {
    let expected = gnu_find_output(args, false);
    let actual = findoxide_output(args, 4, false);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
}

pub fn assert_matches_gnu_exact_with_env(args: &[OsString]) {
    let expected = gnu_find_output(args, true);
    let actual = findoxide_output(args, 1, true);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

pub fn assert_matches_gnu_as_sets_with_env(args: &[OsString]) {
    let expected = gnu_find_output(args, true);
    let actual = findoxide_output(args, 4, true);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
}

pub fn assert_matches_gnu_regex_outcome(args: &[OsString]) {
    let expected = gnu_find_output(args, false);
    let actual = findoxide_output(args, 1, false);

    assert_eq!(actual.status.success(), expected.status.success(), "args: {:?}", args);
    assert_eq!(actual.status.code(), expected.status.code(), "args: {:?}", args);
    assert_eq!(actual.stdout, expected.stdout, "args: {:?}", args);

    if expected.status.success() {
        assert_eq!(actual.stderr, expected.stderr, "args: {:?}", args);
    } else {
        assert!(!expected.stderr.is_empty(), "args: {:?}", args);
        assert!(!actual.stderr.is_empty(), "args: {:?}", args);
    }
}

pub fn assert_matches_gnu_regex_outcome_as_sets(args: &[OsString]) {
    let expected = gnu_find_output(args, false);
    let actual = findoxide_output(args, 4, false);

    assert_eq!(actual.status.success(), expected.status.success(), "args: {:?}", args);
    assert_eq!(actual.status.code(), expected.status.code(), "args: {:?}", args);
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout), "args: {:?}", args);

    if expected.status.success() {
        assert_eq!(lines(&actual.stderr), lines(&expected.stderr), "args: {:?}", args);
    } else {
        assert!(!expected.stderr.is_empty(), "args: {:?}", args);
        assert!(!actual.stderr.is_empty(), "args: {:?}", args);
    }
}

pub fn assert_file_output_matches_gnu_with_env(
    args: &[OsString],
    action: &str,
    workers: usize,
    output_name: &str,
    trailing_args: &[&str],
) {
    let out_dir = tempdir().unwrap();
    let gnu_out = out_dir.path().join(format!("gnu-{output_name}"));
    let oxide_out = out_dir.path().join(format!("oxide-{output_name}"));

    let mut expected_command = Command::new("find");
    apply_common_env(&mut expected_command);
    let expected = expected_command
        .args(args)
        .arg(action)
        .arg(&gnu_out)
        .args(trailing_args)
        .output()
        .unwrap();

    let mut actual_command = Command::cargo_bin("findoxide").unwrap();
    actual_command.env("FINDOXIDE_WORKERS", workers.to_string());
    apply_common_env(&mut actual_command);
    let actual = actual_command
        .args(args)
        .arg(action)
        .arg(&oxide_out)
        .args(trailing_args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(fs::read(&oxide_out).unwrap(), fs::read(&gnu_out).unwrap());
}

pub fn normalize_warning_program(bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| {
            line.strip_prefix("findoxide: ")
                .or_else(|| line.strip_prefix("find: "))
                .unwrap_or(line)
                .to_string()
        })
        .collect()
}
