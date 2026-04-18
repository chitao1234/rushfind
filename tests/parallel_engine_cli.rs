mod support;

use std::fs;
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn parallel_v2_print_matches_set_contract() {
    let root = tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-print".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 3);
}

#[test]
fn parallel_v2_printf_replays_each_record_atomically() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "BEGIN:%p\\nEND:%p\\n".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(lines.len(), 4);
    assert!(
        lines
            .chunks_exact(2)
            .all(|chunk| { chunk[0].starts_with("BEGIN:") && chunk[1].starts_with("END:") })
    );
}

#[test]
fn parallel_v2_prune_keeps_the_preorder_subtree_boundary() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("skip")).unwrap();
    fs::write(root.path().join("skip/hidden.txt"), "x\n").unwrap();
    fs::write(root.path().join("keep.txt"), "x\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "skip".into(),
            "-prune".into(),
            "-o".into(),
            "-print".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("keep.txt"));
    assert!(!stdout.contains("hidden.txt"));
}

#[test]
fn parallel_v2_exec_plus_flushes_worker_shards_on_shutdown() {
    let root = tempdir().unwrap();
    for index in 0..40 {
        fs::write(root.path().join(format!("file-{index:02}.txt")), "x\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "B:%s\\n".into(),
            "{}".into(),
            "+".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout.lines().count(), 40);
    assert!(stdout.lines().all(|line| line.starts_with("B:")));
}
