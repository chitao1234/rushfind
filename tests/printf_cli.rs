mod support;

use std::fs;
use std::os::unix::fs::{self as unix_fs, MetadataExt, PermissionsExt};
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn ordered_printf_renders_path_and_depth_directives() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "2".into(),
            "-printf".into(),
            "[%P][%f][%h][%d]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let expected = format!(
        "[src][src][{}][1]\n[src/lib.rs][lib.rs][{}/src][2]\n",
        root.path().display(),
        root.path().display(),
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn ordered_printf_renders_metadata_and_link_directives() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello").unwrap();
    fs::set_permissions(
        root.path().join("file.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    unix_fs::symlink("file.txt", root.path().join("link.txt")).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-printf".into(),
            "[%f][%y][%s][%m][%l]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[file.txt][f][5][640][]"));
    assert!(stdout.contains("[link.txt][l][8][777][file.txt]"));
}

#[test]
fn parallel_printf_replays_each_record_atomically() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "BEGIN:%f\\nEND:%f\\n".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(lines.chunks_exact(2).all(|chunk| {
        let begin = chunk[0].strip_prefix("BEGIN:").unwrap();
        let end = chunk[1].strip_prefix("END:").unwrap();
        begin == end
    }));
}

#[test]
fn ordered_printf_renders_expanded_metadata_directives() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "hello").unwrap();
    fs::set_permissions(
        root.path().join("dir/file.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%H][%P][%i][%n][%D][%b][%k][%M][%u][%U][%g][%G]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let metadata = fs::metadata(root.path().join("dir/file.txt")).unwrap();
    assert!(stdout.contains(&format!("[{}]", root.path().display())));
    assert!(stdout.contains("[dir/file.txt]"));
    assert!(stdout.contains(&format!("[{}]", metadata.ino())));
    assert!(stdout.contains("[-rw-r-----]"));
}

#[test]
fn ordered_printf_formats_alignment_precision_and_gnu_numeric_flags() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path().join("file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%10i][%-10u][%.2F][%010d][%#10m]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['));
    assert!(stdout.ends_with("]\n"));
}

#[test]
fn parallel_printf_keeps_expanded_records_atomic() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "BEGIN:%10i:%-8u\\nEND:%#m:%+d\\n".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(
        lines
            .chunks_exact(2)
            .all(|chunk| chunk[0].starts_with("BEGIN:") && chunk[1].starts_with("END:"))
    );
}
