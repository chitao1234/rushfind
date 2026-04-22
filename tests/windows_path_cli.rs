#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::normalize_stdout_path;
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn print_and_printf_render_backslashes_in_output() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    for args in [
        vec![
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-printf".into(),
            "%p\\n".into(),
        ],
    ] {
        let output = cargo_bin_output_with_timeout(&args, 1, Duration::from_secs(5));
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            normalize_stdout_path(&format!("{}\n", file.display())),
            "{args:?}"
        );
    }
}
