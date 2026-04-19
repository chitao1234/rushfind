#![allow(dead_code, unused_imports)]

pub mod gnu;
pub mod planner;

use assert_cmd::cargo::cargo_bin;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

pub use gnu::{
    PRINTF_TIME_TZ, assert_file_output_matches_gnu_with_env, assert_matches_gnu_as_sets,
    assert_matches_gnu_as_sets_with_env, assert_matches_gnu_exact,
    assert_matches_gnu_exact_with_env, assert_matches_gnu_exact_with_input,
    assert_matches_gnu_regex_outcome, assert_matches_gnu_regex_outcome_as_sets,
    normalize_warning_program,
};
pub use planner::{action_labels, contains_action, contains_predicate, predicate_labels};

pub fn argv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

pub fn path_arg(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
}

pub fn temp_output_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

pub fn lines(bytes: &[u8]) -> BTreeSet<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| line.to_string())
        .collect()
}

pub fn newline_records(bytes: &[u8]) -> BTreeSet<Vec<u8>> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
        .map(|record| record.to_vec())
        .collect()
}

pub fn nul_records(bytes: &[u8]) -> BTreeSet<Vec<u8>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| record.to_vec())
        .collect()
}

pub fn cargo_bin_output_with_timeout(
    args: &[OsString],
    workers: usize,
    timeout: Duration,
) -> Output {
    cargo_bin_output_with_env_timeout(args, workers, &[], timeout)
}

pub fn cargo_bin_output_with_env_timeout(
    args: &[OsString],
    workers: usize,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> Output {
    let mut command = Command::new(cargo_bin("findoxide"));
    command.env("FINDOXIDE_WORKERS", workers.to_string());
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("findoxide did not exit within {:?}", timeout);
        }
    }
}

pub fn cargo_bin_output_with_input_timeout(
    args: &[OsString],
    workers: usize,
    input: &[u8],
    timeout: Duration,
) -> Output {
    let mut command = Command::new(cargo_bin("findoxide"));
    command
        .env("FINDOXIDE_WORKERS", workers.to_string())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    if !input.is_empty() {
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(child.stdin.take());

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("findoxide did not exit within {:?}", timeout);
        }
    }
}

pub fn cargo_bin_output_with_env_and_input_timeout(
    args: &[OsString],
    workers: usize,
    envs: &[(&str, &str)],
    input: &[u8],
    timeout: Duration,
) -> Output {
    let mut command = Command::new(cargo_bin("findoxide"));
    command.env("FINDOXIDE_WORKERS", workers.to_string());
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    if !input.is_empty() {
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(child.stdin.take());

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("findoxide did not exit within {:?}", timeout);
        }
    }
}

pub fn first_available_locale(candidates: &[&str]) -> Option<String> {
    let output = Command::new("locale").arg("-a").output().ok()?;
    let available = String::from_utf8_lossy(&output.stdout);
    candidates.iter().find_map(|candidate| {
        available
            .lines()
            .find(|line| *line == *candidate)
            .map(str::to_string)
    })
}
