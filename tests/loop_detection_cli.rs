#![cfg(unix)]

mod support;

use support::{gnu_find_command, path_arg, rushfind_command_with_workers};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Output;
use tempfile::tempdir;

fn loop_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "content\n").unwrap();
    // Reaching this link under -L would revisit an ancestor directory.
    symlink(root.path(), root.path().join("dir/up")).unwrap();
    root
}

fn run_gnu(args: &[OsString]) -> Option<Output> {
    Some(gnu_find_command()?.args(args).output().unwrap())
}

fn run_rfd(args: &[OsString], workers: usize) -> Output {
    rushfind_command_with_workers(workers)
        .args(args)
        .output()
        .unwrap()
}

fn sorted_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    lines.sort();
    lines
}

fn assert_stdout_matches_gnu(root: &std::path::Path, extra: &[&str], workers: usize) {
    let mut args = vec![OsString::from("-L"), path_arg(root)];
    args.extend(extra.iter().map(OsString::from));

    let Some(expected) = run_gnu(&args) else {
        eprintln!("GNU find is unavailable; skipping");
        return;
    };
    let actual = run_rfd(&args, workers);

    assert_eq!(
        sorted_lines(&actual.stdout),
        sorted_lines(&expected.stdout),
        "stdout differs for {args:?} with {workers} worker(s)"
    );
    assert_eq!(
        actual.status.code(),
        expected.status.code(),
        "exit status differs for {args:?} with {workers} worker(s)"
    );
    assert!(
        !actual.stdout.windows(7).any(|window| window == b"dir/up\n"),
        "the looping entry must not be reported as visited: {:?}",
        String::from_utf8_lossy(&actual.stdout)
    );
}

#[test]
fn pre_order_walk_skips_the_looping_entry_like_gnu_find() {
    let root = loop_tree();

    assert_stdout_matches_gnu(root.path(), &["-print"], 1);
    assert_stdout_matches_gnu(root.path(), &["-print"], 4);
}

#[test]
fn depth_first_walk_skips_the_looping_entry_like_gnu_find() {
    let root = loop_tree();

    assert_stdout_matches_gnu(root.path(), &["-depth", "-print"], 1);
    assert_stdout_matches_gnu(root.path(), &["-depth", "-print"], 4);
}

#[test]
fn loop_diagnostics_still_fail_the_run() {
    let root = loop_tree();
    let args = vec![
        OsString::from("-L"),
        path_arg(root.path()),
        OsString::from("-print"),
    ];

    let actual = run_rfd(&args, 1);

    assert_eq!(actual.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&actual.stderr).contains("loop"),
        "expected a loop diagnostic on stderr, got {:?}",
        String::from_utf8_lossy(&actual.stderr)
    );
}
