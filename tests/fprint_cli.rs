mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn ordered_fprint_creates_an_empty_file_when_nothing_matches() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("keep.txt"), "keep\n").unwrap();
    let out = root.path().join("matches.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "definitely-missing".into(),
            "-fprint".into(),
            path_arg(&out),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read(&out).unwrap(), b"");
}

#[test]
fn ordered_fprint0_writes_nul_terminated_paths() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    let out = root.path().join("hits.bin");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-fprint0".into(),
            path_arg(&out),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read(&out).unwrap().ends_with(&[0]));
}

#[test]
fn ordered_fprint_start_failure_prevents_earlier_exec_side_effects() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    let marker = root.path().join("marker.txt");
    let bad = root.path().join("missing/out.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-exec".into(),
            "sh".into(),
            "-c".into(),
            "printf side >> \"$2\"".into(),
            "sh".into(),
            "{}".into(),
            path_arg(&marker),
            ";".into(),
            "-fprint".into(),
            path_arg(&bad),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!marker.exists());
}

#[test]
fn ordered_fprint_shared_destination_appends_without_retruncating() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    let out = root.path().join("shared.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-fprint".into(),
            path_arg(&out),
            "-fprintf".into(),
            path_arg(&out),
            "[%f]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let bytes = fs::read(&out).unwrap();
    let rendered = String::from_utf8_lossy(&bytes);
    assert!(rendered.contains("alpha.txt"));
    assert!(rendered.contains("[alpha.txt]"));
}

#[test]
fn ordered_fprint_destination_is_visible_when_created_inside_the_tree() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    let out = root.path().join("seen.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "seen.txt".into(),
            "-print".into(),
            "-o".into(),
            "-type".into(),
            "f".into(),
            "-fprint".into(),
            path_arg(&out),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("seen.txt"));
}
