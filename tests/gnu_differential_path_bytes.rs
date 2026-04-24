#![cfg(unix)]

mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;
use std::process::Output;
use support::{gnu_find_output, path_arg, rushfind_command, supports_non_utf8_temp_paths};
use tempfile::tempdir;

fn os(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(os(bytes))
}

fn run_gnu(args: &[OsString]) -> Option<Output> {
    gnu_find_output(args, true)
}

fn run_fox(args: &[OsString]) -> Output {
    rushfind_command()
        .env("RUSHFIND_WORKERS", "1")
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .unwrap()
}

fn build_non_utf8_tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempdir().unwrap();
    let file = root.path().join(path_from_bytes(b"ReadMe-\xff.TXT"));
    let link = root.path().join(path_from_bytes(b"sym-\xfd"));
    fs::write(&file, "demo\n").unwrap();
    unix_fs::symlink(path_from_bytes(b"TarGet-\xfe.bin"), &link).unwrap();
    (root, file, link)
}

#[test]
fn print_and_printf_match_gnu_for_non_utf8_paths() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let (root, _, _) = build_non_utf8_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-type".into(),
            "l".into(),
            "-printf".into(),
            "[%p][%P][%H][%f][%h][%l]\\n".into(),
        ],
    ] {
        let Some(gnu) = run_gnu(&args) else {
            return;
        };
        let fox = run_fox(&args);
        assert_eq!(fox.status.code(), gnu.status.code());
        assert_eq!(fox.stdout, gnu.stdout);
        assert_eq!(fox.stderr, gnu.stderr);
    }
}

#[test]
fn name_path_and_lname_match_gnu_for_non_utf8_operands() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let (root, file, _) = build_non_utf8_tree();
    let mut ipath_pattern = root.path().as_os_str().as_bytes().to_vec();
    ipath_pattern.extend_from_slice(b"/readme-\xff.txt");

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-iname".into(),
            os(b"readme-\xff.txt"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-path".into(),
            file.as_os_str().to_os_string(),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-ipath".into(),
            os(&ipath_pattern),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-lname".into(),
            os(b"TarGet-\xfe.bin"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-ilname".into(),
            os(b"target-\xfe.bin"),
            "-print0".into(),
        ],
    ] {
        let Some(gnu) = run_gnu(&args) else {
            return;
        };
        let fox = run_fox(&args);
        assert_eq!(fox.status.code(), gnu.status.code(), "args: {:?}", args);
        assert_eq!(fox.stdout, gnu.stdout, "args: {:?}", args);
        assert_eq!(fox.stderr, gnu.stderr, "args: {:?}", args);
    }
}

#[test]
fn fprint_matches_gnu_for_non_utf8_paths() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let (root, _, _) = build_non_utf8_tree();
    let outputs = tempdir().unwrap();
    let gnu_out = outputs.path().join("gnu.txt");
    let fox_out = outputs.path().join("fox.txt");

    let Some(gnu) = run_gnu(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-fprint".into(),
        path_arg(&gnu_out),
    ]) else {
        return;
    };
    let fox = run_fox(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-fprint".into(),
        path_arg(&fox_out),
    ]);

    assert_eq!(fox.status.code(), gnu.status.code());
    assert_eq!(fox.stderr, gnu.stderr);
    assert_eq!(fs::read(&fox_out).unwrap(), fs::read(&gnu_out).unwrap());
}

#[test]
fn fprint0_matches_gnu_for_non_utf8_paths() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let (root, _, _) = build_non_utf8_tree();
    let outputs = tempdir().unwrap();
    let gnu_out = outputs.path().join("gnu.bin");
    let fox_out = outputs.path().join("fox.bin");

    let Some(gnu) = run_gnu(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-fprint0".into(),
        path_arg(&gnu_out),
    ]) else {
        return;
    };
    let fox = run_fox(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-fprint0".into(),
        path_arg(&fox_out),
    ]);

    assert_eq!(fox.status.code(), gnu.status.code());
    assert_eq!(fox.stderr, gnu.stderr);
    assert_eq!(fs::read(&fox_out).unwrap(), fs::read(&gnu_out).unwrap());
}

#[test]
fn fprintf_matches_gnu_for_non_utf8_paths() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let (root, _, _) = build_non_utf8_tree();
    let outputs = tempdir().unwrap();
    let gnu_out = outputs.path().join("gnu-report.txt");
    let fox_out = outputs.path().join("fox-report.txt");

    let Some(gnu) = run_gnu(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-type".into(),
        "l".into(),
        "-fprintf".into(),
        path_arg(&gnu_out),
        "[%p][%P][%H][%f][%h][%l]\\n".into(),
    ]) else {
        return;
    };
    let fox = run_fox(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-type".into(),
        "l".into(),
        "-fprintf".into(),
        path_arg(&fox_out),
        "[%p][%P][%H][%f][%h][%l]\\n".into(),
    ]);

    assert_eq!(fox.status.code(), gnu.status.code());
    assert_eq!(fox.stderr, gnu.stderr);
    assert_eq!(fs::read(&fox_out).unwrap(), fs::read(&gnu_out).unwrap());
}
