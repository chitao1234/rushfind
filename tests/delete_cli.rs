mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

fn directory_not_empty_fragment() -> &'static str {
    #[cfg(windows)]
    {
        "directory is not empty"
    }

    #[cfg(unix)]
    {
        "Directory not empty"
    }
}

#[test]
fn ordered_delete_removes_entries_reached_by_the_expression() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::create_dir(tree.join("empty-dir")).unwrap();
    fs::write(tree.join("leaf.txt"), "leaf\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(&tree),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(fs::read_dir(&tree).unwrap().next().is_none());
}

#[test]
fn ordered_print_then_delete_emits_paths_before_removal() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("alpha.txt"), "alpha\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(&tree),
            "-mindepth".into(),
            "1".into(),
            "-print".into(),
            "-delete".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("alpha.txt")
    );
    assert!(fs::read_dir(&tree).unwrap().next().is_none());
}

#[test]
fn ordered_delete_failure_falls_through_or_branch_and_sets_exit_one() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::create_dir(tree.join("dir")).unwrap();
    fs::write(tree.join("dir/child.txt"), "child\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(&tree),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "d".into(),
            "-delete".into(),
            "-o".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("dir"));
    assert!(stdout.contains("child.txt"));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .to_ascii_lowercase()
            .contains(&directory_not_empty_fragment().to_ascii_lowercase())
    );
}

#[test]
fn parallel_delete_removes_the_same_tree_shape_with_relaxed_ordering() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::create_dir(tree.join("nested")).unwrap();
    fs::write(tree.join("nested/file.txt"), "child\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(&tree),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read_dir(&tree).unwrap().next().is_none());
}

#[test]
fn parallel_depth_delete_keeps_descendant_before_parent_behavior_with_multiple_evaluators() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "child\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[path_arg(root.path()), "-delete".into()],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(!root.path().join("dir").exists());
}

#[test]
fn parallel_v2_delete_removes_the_same_tree_shape_under_the_override() {
    let root = tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::create_dir(tree.join("nested")).unwrap();
    fs::write(tree.join("nested/file.txt"), "child\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(&tree),
            "-mindepth".into(),
            "1".into(),
            "-delete".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read_dir(&tree).unwrap().next().is_none());
}
