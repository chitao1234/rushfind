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

pub fn cargo_bin_output_with_timeout(
    args: &[OsString],
    workers: usize,
    timeout: Duration,
) -> Output {
    let mut child = Command::new(cargo_bin("findoxide"))
        .env("FINDOXIDE_WORKERS", workers.to_string())
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

pub fn cargo_bin_output_with_engine(
    args: &[OsString],
    workers: usize,
    engine: &str,
    timeout: Duration,
) -> Output {
    let mut child = Command::new(cargo_bin("findoxide"))
        .env("FINDOXIDE_WORKERS", workers.to_string())
        .env("FINDOXIDE_PARALLEL_ENGINE", engine)
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
