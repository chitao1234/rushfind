mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use support::path_arg;
use tempfile::tempdir;

fn os(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(os(bytes))
}

fn run_gnu(args: &[OsString]) -> Output {
    Command::new("find")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .unwrap()
}

fn run_fox(args: &[OsString]) -> Output {
    Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .unwrap()
}

fn build_non_utf8_regex_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join(path_from_bytes(b"ReadMe-\xff.TXT")),
        "target\n",
    )
    .unwrap();
    fs::write(
        root.path().join(path_from_bytes(b"ReadMe-\xfe.TXT")),
        "other\n",
    )
    .unwrap();
    fs::write(root.path().join("README.TXT"), "ascii\n").unwrap();
    root
}

#[test]
fn regex_and_iregex_match_gnu_for_non_utf8_operands() {
    let root = build_non_utf8_regex_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regex".into(),
            os(b".*/ReadMe-\xff\\.TXT"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-iregex".into(),
            os(b".*/readme-\xff\\.txt"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            os(b".*/ReadMe-\xff\\.TXT"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            os(b".*/ReadMe-\xff\\.TXT"),
            "-print0".into(),
        ],
    ] {
        let gnu = run_gnu(&args);
        let fox = run_fox(&args);
        assert_eq!(fox.status.code(), gnu.status.code(), "args: {:?}", args);
        assert_eq!(fox.stdout, gnu.stdout, "args: {:?}", args);
        assert_eq!(fox.stderr, gnu.stderr, "args: {:?}", args);
    }
}

#[test]
fn regex_foundation_matrix_gnu_and_pcre2_preserve_non_utf8_subject_matching() {
    let root = build_non_utf8_regex_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            os(b".*/ReadMe-\xff\\.TXT"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "pcre2".into(),
            "-regex".into(),
            OsString::from(".*/ReadMe-\\xFF\\.TXT"),
            "-print0".into(),
        ],
    ] {
        let fox = run_fox(&args);
        assert!(fox.status.success(), "args: {:?}", args);
        assert!(!fox.stdout.is_empty(), "args: {:?}", args);
    }
}

fn build_non_utf8_emacs_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join(path_from_bytes(b"pair-\xff\xff")),
        "repeat\n",
    )
    .unwrap();
    fs::write(
        root.path().join(path_from_bytes(b"pair-\xff\xfe")),
        "mixed\n",
    )
    .unwrap();
    fs::write(root.path().join("pair-ascii"), "ascii\n").unwrap();
    root
}

#[test]
fn regex_emacs_followup_matrix_emacs_literal_byte_patterns_match_gnu_for_non_utf8_operands() {
    let root = build_non_utf8_emacs_tree();
    let args = vec![
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-regextype".into(),
        "emacs".into(),
        "-regex".into(),
        os(b".*/pair-\xff\xff"),
        "-print0".into(),
    ];

    let gnu = run_gnu(&args);
    let fox = run_fox(&args);
    assert_eq!(fox.status.code(), gnu.status.code(), "args: {:?}", args);
    assert_eq!(fox.stdout, gnu.stdout, "args: {:?}", args);
    assert_eq!(fox.stderr, gnu.stderr, "args: {:?}", args);
}

fn build_non_utf8_gnu_hardening_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join(path_from_bytes(b"slot-\xff")), "hi\n").unwrap();
    fs::write(root.path().join(path_from_bytes(b"slot-\\")), "slash\n").unwrap();
    fs::write(
        root.path().join(path_from_bytes(b"pair-\xff\xff")),
        "repeat\n",
    )
    .unwrap();
    root
}

#[test]
fn gnu_hardening_bytes_literal_backslash_and_high_bytes_match_gnu() {
    let root = build_non_utf8_gnu_hardening_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            os(b".*/slot-[\\\\\xff]"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            os(b".*/pair-\xff\xff"),
            "-print0".into(),
        ],
    ] {
        let gnu = run_gnu(&args);
        let fox = run_fox(&args);
        assert_eq!(fox.status.code(), gnu.status.code(), "args: {:?}", args);
        assert_eq!(fox.stdout, gnu.stdout, "args: {:?}", args);
        assert_eq!(fox.stderr, gnu.stderr, "args: {:?}", args);
    }
}
