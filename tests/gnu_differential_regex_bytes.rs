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
    fs::write(root.path().join(path_from_bytes(b"ReadMe-\xff.TXT")), "target\n").unwrap();
    fs::write(root.path().join(path_from_bytes(b"ReadMe-\xfe.TXT")), "other\n").unwrap();
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
