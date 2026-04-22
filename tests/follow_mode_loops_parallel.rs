#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs as unix_fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, lines, path_arg};
use tempfile::tempdir;

#[test]
fn parallel_logical_mode_matches_ordered_results_and_reports_loops() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::create_dir(root.path().join("real/sub")).unwrap();
    fs::write(root.path().join("real/sub/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-a")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-b")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("real/loop")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "file.txt".into(),
    ];

    let ordered = cargo_bin_output_with_timeout(&args, 1, Duration::from_secs(2));
    let parallel = cargo_bin_output_with_timeout(&args, 4, Duration::from_secs(2));

    assert_eq!(ordered.status.code(), Some(1));
    assert_eq!(parallel.status.code(), Some(1));
    assert_eq!(lines(&ordered.stdout), lines(&parallel.stdout));

    let stderr = String::from_utf8(parallel.stderr).unwrap();
    assert!(stderr.contains("filesystem loop detected"));
}
