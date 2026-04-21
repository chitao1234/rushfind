mod support;

use rushfind::time::Timestamp;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{self as unix_fs, MetadataExt, PermissionsExt};
use std::time::Duration;
use support::{cargo_bin_output_with_env_timeout, cargo_bin_output_with_timeout, path_arg};
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
    let link_mode = fs::symlink_metadata(root.path().join("link.txt"))
        .unwrap()
        .mode()
        & 0o7777;
    assert!(stdout.contains("[file.txt][f][5][640][]"));
    assert!(stdout.contains(&format!("[link.txt][l][8][{:o}][file.txt]", link_mode)));
}

#[test]
fn ordered_printf_decodes_gnu_literal_escapes() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path().join("file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "A\\aB\\bC\\fD\\nE\\rF\\tG\\vH\\101\\040\\0123\\400".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.stdout, b"A\x07B\x08C\x0cD\nE\rF\tG\x0bHA \n3\0");
    assert!(output.stderr.is_empty());
}

#[test]
fn ordered_printf_backslash_c_stops_only_the_current_printf_action() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path().join("file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "A\\cB".into(),
            "-printf".into(),
            "Z".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let expected_path = format!("{}\n", root.path().join("file.txt").display());
    assert_eq!(
        output.stdout,
        [b"AZ".as_slice(), expected_path.as_bytes()].concat()
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn ordered_printf_unknown_escapes_warn_per_occurrence_and_render_literally() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path().join("file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "X\\qY\\xZ".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.stdout, b"X\\qY\\xZ");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning: unrecognized escape `\\q'"));
    assert!(stderr.contains("warning: unrecognized escape `\\x'"));
}

#[test]
fn ordered_printf_unknown_escape_warnings_are_emitted_even_when_no_entries_match() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            "definitely-no-match".into(),
            "-printf".into(),
            "X\\q".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("warning: unrecognized escape `\\q'"));
}

#[test]
fn ordered_fprintf_decodes_gnu_literal_escapes_and_honors_backslash_c_per_action() {
    let root = tempdir().unwrap();
    let output_path = root.path().join("out.txt");
    fs::write(root.path().join("file.txt"), "x").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path().join("file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-fprintf".into(),
            path_arg(output_path.as_path()),
            "A\\a\\101\\cB".into(),
            "-fprintf".into(),
            path_arg(output_path.as_path()),
            "Z".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert!(output.status.success());
    assert_eq!(fs::read(output_path).unwrap(), b"A\x07AZ");
    assert!(output.stderr.is_empty());
}

#[test]
fn ordered_printf_renders_symlink_target_type_directive() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("file.txt"), "hello").unwrap();
    unix_fs::symlink("file.txt", root.path().join("file-link")).unwrap();
    unix_fs::symlink("dir", root.path().join("dir-link")).unwrap();
    unix_fs::symlink("missing", root.path().join("missing-link")).unwrap();
    unix_fs::symlink("loop", root.path().join("loop")).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-printf".into(),
            "%f:%y:%Y\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("file.txt:f:f"));
    assert!(stdout.contains("dir:d:d"));
    assert!(stdout.contains("file-link:l:f"));
    assert!(stdout.contains("dir-link:l:d"));
    assert!(stdout.contains("missing-link:l:N"));
    assert!(stdout.contains("loop:l:L"));
}

#[test]
fn ordered_printf_renders_sparseness_for_dense_and_sparse_files() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("zero"), []).unwrap();
    fs::write(root.path().join("one"), b"x").unwrap();
    fs::write(root.path().join("tiny"), b"xyz").unwrap();
    fs::write(root.path().join("fivek"), vec![b'x'; 5000]).unwrap();
    let mut holey = fs::File::create(root.path().join("holey")).unwrap();
    holey.seek(SeekFrom::Start(8191)).unwrap();
    holey.write_all(b"x").unwrap();
    let trunc8k = fs::File::create(root.path().join("trunc8k")).unwrap();
    trunc8k.set_len(8192).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
            "-printf".into(),
            "%f:%s:%b:%S\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    for name in ["zero", "one", "tiny", "fivek", "holey", "trunc8k"] {
        let metadata = fs::metadata(root.path().join(name)).unwrap();
        assert!(stdout.contains(&format!(
            "{name}:{}:{}:{}",
            metadata.len(),
            metadata.blocks(),
            format_sparseness_ascii(metadata.len(), metadata.blocks()),
        )));
    }
}

fn format_sparseness_ascii(size: u64, blocks: u64) -> String {
    if size == 0 {
        return "1".to_string();
    }

    format_six_sigfigs_ascii((blocks as f64 * 512.0) / size as f64)
}

fn format_six_sigfigs_ascii(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }

    let exponent = value.abs().log10().floor() as i32;
    if !(-4..6).contains(&exponent) {
        return trim_ascii_float(format!("{value:.5e}"));
    }

    let precision = (5 - exponent).max(0) as usize;
    trim_ascii_float(format!("{value:.precision$}", precision = precision))
}

fn trim_ascii_float(text: String) -> String {
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            format!("{}e{}", trim_ascii_decimal(mantissa), exponent)
        }
        None => trim_ascii_decimal(&text),
    }
}

fn trim_ascii_decimal(text: &str) -> String {
    let mut out = text.to_string();
    while out.contains('.') && out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
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

#[test]
fn ordered_printf_renders_time_directives_in_a_fixed_local_timezone() {
    let root = tempdir().unwrap();
    let path = root.path().join("stamp.txt");
    fs::write(&path, "hello").unwrap();
    set_file_times(
        &path,
        Timestamp::new(1_709_528_767, 123_456_789),
        Timestamp::new(1_709_528_767, 123_456_789),
    );

    let output = cargo_bin_output_with_env_timeout(
        &[
            path_arg(&path),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%t][%TY-%Tm-%Td][%TH:%TM:%TS][%T@][%T+][%.3Ta][%10Ta]\\n".into(),
        ],
        1,
        &[("TZ", "Asia/Shanghai"), ("LC_ALL", "C")],
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[Mon Mar  4 13:06:07.1234567890 2024][2024-03-04][13:06:07.1234567890][1709528767.1234567890][2024-03-04+13:06:07.1234567890][Mon][       Mon]\n"
    );
}

fn set_file_times(path: &std::path::Path, atime: Timestamp, mtime: Timestamp) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: atime.seconds as libc::time_t,
            tv_nsec: atime.nanos.into(),
        },
        libc::timespec {
            tv_sec: mtime.seconds as libc::time_t,
            tv_nsec: mtime.nanos.into(),
        },
    ];

    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0);
}

#[test]
fn parallel_printf_keeps_time_records_atomic() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_env_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "BEGIN:%f:%TY-%Tm-%Td\\nEND:%f:%T@\\n".into(),
        ],
        4,
        &[("TZ", "Asia/Shanghai"), ("LC_ALL", "C")],
        Duration::from_secs(5),
    );

    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(lines.chunks_exact(2).all(|chunk| {
        let begin_name = chunk[0].split(':').nth(1).unwrap();
        let end_name = chunk[1].split(':').nth(1).unwrap();
        begin_name == end_name
    }));
}
