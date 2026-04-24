#![cfg(unix)]

mod support;

use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use support::supports_non_utf8_temp_paths;
use support::{path_arg, rushfind_command};
use tempfile::tempdir;

#[test]
fn regex_foundation_matrix_pcre2_accepts_raw_syntax() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "lib\n").unwrap();
    fs::write(root.path().join("docs/Guide.txt"), "guide\n").unwrap();

    let output = rushfind_command()
        .env("RUSHFIND_WORKERS", "1")
        .args([
            path_arg(root.path()),
            "-regextype".into(),
            "pcre2".into(),
            "-regex".into(),
            ".*/(?:src|docs)/.+\\.(?:rs|txt)".into(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn regex_foundation_matrix_pcre2_reports_invalid_patterns() {
    let root = tempdir().unwrap();
    let output = rushfind_command()
        .env("RUSHFIND_WORKERS", "1")
        .args([
            path_arg(root.path()),
            "-regextype".into(),
            "pcre2".into(),
            "-regex".into(),
            "(?".into(),
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("failed to compile pcre2 regex")
    );
}

#[cfg(unix)]
#[test]
fn regex_foundation_matrix_pcre2_matches_non_utf8_candidates_via_hex_escape() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let root = tempdir().unwrap();
    let path = root
        .path()
        .join(std::path::PathBuf::from(OsString::from_vec(vec![
            b'f', b'o', b'o', 0xff,
        ])));
    fs::write(&path, "target\n").unwrap();

    let output = rushfind_command()
        .env("RUSHFIND_WORKERS", "1")
        .args([
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "pcre2".into(),
            "-regex".into(),
            ".*/foo\\xFF".into(),
            "-print0".into(),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}
