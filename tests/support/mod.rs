#![allow(dead_code)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

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
