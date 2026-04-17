mod support;

use std::fs;
use std::os::unix::fs::{self as unix_fs, PermissionsExt};
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
