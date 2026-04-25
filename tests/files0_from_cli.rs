#![cfg(unix)]

mod support;

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::time::Duration;
use support::{
    cargo_bin_output_with_input_timeout, cargo_bin_output_with_timeout, lines, path_arg,
};
use tempfile::tempdir;

fn write_nul_list(path: &std::path::Path, roots: &[&std::path::Path]) {
    let mut file = fs::File::create(path).unwrap();
    for root in roots {
        file.write_all(root.as_os_str().as_encoded_bytes()).unwrap();
        file.write_all(&[0]).unwrap();
    }
}

#[test]
fn files0_from_file_supplies_start_paths() {
    let root = tempdir().unwrap();
    let alpha = root.path().join("alpha");
    let beta = root.path().join("beta");
    fs::create_dir(&alpha).unwrap();
    fs::create_dir(&beta).unwrap();
    fs::write(alpha.join("one.txt"), b"one").unwrap();
    fs::write(beta.join("two.txt"), b"two").unwrap();
    let list = root.path().join("roots.list");
    write_nul_list(&list, &[alpha.as_path(), beta.as_path()]);

    let output = cargo_bin_output_with_timeout(
        &[
            "-files0-from".into(),
            path_arg(&list),
            "-maxdepth".into(),
            "0".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        lines(&output.stdout),
        [
            alpha.to_string_lossy().to_string(),
            beta.to_string_lossy().to_string(),
        ]
        .into_iter()
        .collect()
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn empty_files0_from_file_exits_successfully_without_default_path() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("visible.txt"), b"visible").unwrap();
    let list = root.path().join("empty.list");
    fs::write(&list, []).unwrap();

    let output = cargo_bin_output_with_timeout(
        &["-files0-from".into(), path_arg(&list), "-print".into()],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn zero_length_files0_from_items_are_rejected() {
    let root = tempdir().unwrap();
    let list = root.path().join("bad.list");
    fs::write(&list, [0]).unwrap();

    let output = cargo_bin_output_with_timeout(
        &["-files0-from".into(), path_arg(&list), "-print".into()],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("zero-length file name"), "{stderr}");
}

#[test]
fn files0_from_stdin_supplies_start_paths() {
    let root = tempdir().unwrap();
    let alpha = root.path().join("alpha");
    fs::create_dir(&alpha).unwrap();
    fs::write(alpha.join("one.txt"), b"one").unwrap();
    let mut input = alpha.as_os_str().as_encoded_bytes().to_vec();
    input.push(0);

    let output = cargo_bin_output_with_input_timeout(
        &[
            "-files0-from".into(),
            "-".into(),
            "-maxdepth".into(),
            "0".into(),
            "-print".into(),
        ],
        1,
        &input,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", alpha.display())
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn files0_from_rejects_explicit_command_line_paths() {
    let root = tempdir().unwrap();
    let list = root.path().join("roots.list");
    write_nul_list(&list, &[root.path()]);

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-files0-from".into(),
            path_arg(&list),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("extra operand") || stderr.contains("-files0-from"),
        "{stderr}"
    );
}

#[test]
fn files0_from_stdin_rejects_ok_and_okdir() {
    for action in ["-ok", "-okdir"] {
        let output = cargo_bin_output_with_input_timeout(
            &[
                "-files0-from".into(),
                "-".into(),
                action.into(),
                "echo".into(),
                "{}".into(),
                ";".into(),
            ],
            1,
            b"/tmp\0",
            Duration::from_secs(5),
        );

        assert_eq!(output.status.code(), Some(1), "{action}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("standard input"), "{action}: {stderr}");
        assert!(stderr.contains(action), "{action}: {stderr}");
    }
}

#[test]
fn files0_from_preserves_non_utf8_unix_paths() {
    if !support::supports_non_utf8_temp_paths() {
        return;
    }

    let root = tempdir().unwrap();
    let name = OsString::from_vec(b"raw-\xff".to_vec());
    let path = root.path().join(&name);
    fs::write(&path, b"raw").unwrap();
    let list = root.path().join("roots.list");
    write_nul_list(&list, &[path.as_path()]);

    let output = cargo_bin_output_with_timeout(
        &[
            "-files0-from".into(),
            path_arg(&list),
            "-maxdepth".into(),
            "0".into(),
            "-print0".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        path.as_os_str()
            .as_encoded_bytes()
            .iter()
            .copied()
            .chain([0])
            .collect::<Vec<_>>()
    );
}
