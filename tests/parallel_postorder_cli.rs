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
fn parallel_v2_delete_keeps_descendant_before_parent_when_children_are_chunked() {
    let root = tempdir().unwrap();
    let parent = root.path().join("parent");
    fs::create_dir(&parent).unwrap();
    for child_index in 0..96 {
        let child = parent.join(format!("child-{child_index:03}"));
        fs::create_dir(&child).unwrap();
        for leaf_index in 0..4 {
            fs::write(child.join(format!("leaf-{leaf_index:02}.txt")), "x\n").unwrap();
        }
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        4,
        Duration::from_secs(10),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
