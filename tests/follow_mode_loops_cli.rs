mod support;

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, lines, path_arg};
use tempfile::tempdir;

#[test]
fn logical_mode_reports_loop_without_hanging() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::create_dir(root.path().join("real/sub")).unwrap();
    fs::write(root.path().join("real/sub/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("real/loop")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "file.txt".into(),
    ];

    let output = cargo_bin_output_with_timeout(&args, 1, Duration::from_secs(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout,
        format!("{}\n", root.path().join("real/sub/file.txt").display())
    );
    assert!(stderr.contains("filesystem loop detected"));
    assert!(stderr.contains(&root.path().join("real/loop").display().to_string()));
}

#[test]
fn logical_mode_reports_mutual_loop_without_hanging() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("a/keep")).unwrap();
    fs::create_dir_all(root.path().join("b/keep")).unwrap();
    fs::write(root.path().join("a/keep/file-a.txt"), "a\n").unwrap();
    fs::write(root.path().join("b/keep/file-b.txt"), "b\n").unwrap();
    unix_fs::symlink(root.path().join("b"), root.path().join("a/to-b")).unwrap();
    unix_fs::symlink(root.path().join("a"), root.path().join("b/to-a")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "*.txt".into(),
    ];

    let output = cargo_bin_output_with_timeout(&args, 1, Duration::from_secs(2));
    let stdout = lines(&output.stdout);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout,
        BTreeSet::from([
            root.path().join("a/keep/file-a.txt").display().to_string(),
            root.path()
                .join("a/to-b/keep/file-b.txt")
                .display()
                .to_string(),
            root.path().join("b/keep/file-b.txt").display().to_string(),
            root.path()
                .join("b/to-a/keep/file-a.txt")
                .display()
                .to_string(),
        ])
    );
    assert!(stderr.contains("filesystem loop detected"));
    assert!(stderr.contains(&root.path().join("a/to-b/to-a").display().to_string()));
}
