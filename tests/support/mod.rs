#![allow(dead_code)]

use assert_cmd::cargo::cargo_bin;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

pub fn argv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

pub fn path_arg(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
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
