mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn ordered_print_then_quit_emits_exactly_one_match() {
    let root = tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.path().join(name), format!("{name}\n")).unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-print".into(),
            "-quit".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
}

#[test]
fn ordered_quit_before_print_emits_nothing() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-quit".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn ordered_exec_plus_flushes_before_quit() {
    let root = tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.path().join(name), format!("{name}\n")).unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "Q:%s\\n".into(),
            "{}".into(),
            "+".into(),
            "-quit".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(!stdout.is_empty());
    assert!(stdout.lines().all(|line| line.starts_with("Q:")));
}

#[test]
fn parallel_print_quit_stops_before_visiting_the_entire_tree() {
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("file-{index:03}.txt")), "x\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-print".into(),
            "-quit".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let line_count = String::from_utf8(output.stdout).unwrap().lines().count();
    assert_eq!(output.status.code(), Some(0));
    assert!(line_count >= 1);
    assert!(line_count < 200);
}

#[test]
fn parallel_exec_plus_quit_flushes_buffered_batches_before_exit() {
    let root = tempdir().unwrap();
    for index in 0..200 {
        fs::write(root.path().join(format!("file-{index:03}.txt")), "x\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "P:%s\\n".into(),
            "{}".into(),
            "+".into(),
            "-quit".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let line_count = stdout.lines().count();
    assert_eq!(output.status.code(), Some(0));
    assert!(line_count >= 1);
    assert!(line_count < 200);
    assert!(stdout.lines().all(|line| line.starts_with("P:")));
}
