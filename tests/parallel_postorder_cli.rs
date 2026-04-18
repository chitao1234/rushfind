mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn parallel_v2_delete_keeps_descendant_before_parent_behavior() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "x\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(!root.path().join("dir").exists());
}

#[test]
fn parallel_v2_depth_prune_stays_truthy_but_does_not_block_descendants() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("keep")).unwrap();
    fs::write(root.path().join("keep/file.txt"), "x\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-depth".into(),
            "-name".into(),
            "keep".into(),
            "-prune".into(),
            "-o".into(),
            "-print".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("keep/file.txt")
    );
}

#[test]
fn parallel_v2_delete_keeps_descendant_before_parent_behavior_when_parent_spills_children() {
    let root = tempdir().unwrap();
    let parent = root.path().join("parent");
    fs::create_dir(&parent).unwrap();
    for index in 0..40 {
        let child = parent.join(format!("child-{index:02}"));
        fs::create_dir(&child).unwrap();
        fs::write(child.join("leaf.txt"), "x\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(!parent.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
