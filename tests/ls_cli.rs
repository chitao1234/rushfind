mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, newline_records, path_arg, temp_output_path};
use tempfile::tempdir;

#[test]
fn ordered_fls_creates_an_empty_file_when_nothing_matches() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("keep.txt"), "keep\n").unwrap();
    let out = root.path().join("listing.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "definitely-missing".into(),
            "-fls".into(),
            path_arg(&out),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read(&out).unwrap(), b"");
}

#[test]
fn ordered_fls_startup_failure_prevents_earlier_exec_side_effects() {
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
            "-fls".into(),
            path_arg(&bad),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!marker.exists());
}

#[test]
fn ordered_fls_shared_destination_appends_without_retruncating() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    let out = root.path().join("shared.txt");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-fls".into(),
            path_arg(&out),
            "-fprintf".into(),
            path_arg(&out),
            "[%f]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let rendered = String::from_utf8_lossy(&fs::read(&out).unwrap()).into_owned();
    assert!(rendered.contains("alpha.txt"));
    assert!(rendered.contains("[alpha.txt]"));
}

#[test]
fn parallel_fls_keeps_each_record_atomic_per_destination() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha file.txt"), "a\n").unwrap();
    #[cfg(unix)]
    fs::write(root.path().join("beta\tfile.txt"), "b\n").unwrap();
    #[cfg(windows)]
    fs::write(root.path().join("beta file 2.txt"), "b\n").unwrap();
    let (_out_dir, out) = temp_output_path("parallel.ls");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-fls".into(),
            path_arg(&out),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let records = newline_records(&fs::read(&out).unwrap());
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.windows(4).any(|w| w == b".txt"))
    );
}
