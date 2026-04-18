mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::{cargo_bin_output_with_timeout, newline_records, nul_records, path_arg};
use tempfile::tempdir;

fn os(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(os(bytes))
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

#[test]
fn ordered_print_and_fprint_surfaces_preserve_non_utf8_paths() {
    let root = tempdir().unwrap();
    let file = root.path().join(path_from_bytes(b"ReadMe-\xff.TXT"));
    let out_txt = root.path().join("hits.txt");
    let out_bin = root.path().join("hits.bin");
    fs::write(&file, "demo\n").unwrap();

    let print = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(print.status.code(), Some(0));
    assert_eq!(
        newline_records(&print.stdout),
        std::iter::once(path_bytes(&file)).collect()
    );

    let print0 = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-print0".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(print0.status.code(), Some(0));
    assert_eq!(print0.stdout, [path_bytes(&file), vec![0]].concat());

    let fprint = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-fprint".into(),
            path_arg(&out_txt),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(fprint.status.code(), Some(0));
    assert_eq!(
        newline_records(&fs::read(&out_txt).unwrap()),
        std::iter::once(path_bytes(&file)).collect()
    );

    let fprint0 = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-fprint0".into(),
            path_arg(&out_bin),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(fprint0.status.code(), Some(0));
    assert_eq!(
        fs::read(&out_bin).unwrap(),
        [path_bytes(&file), vec![0]].concat()
    );
}

#[test]
fn name_and_path_families_accept_non_utf8_operands() {
    let root = tempdir().unwrap();
    let file = root.path().join(path_from_bytes(b"ReadMe-\xff.TXT"));
    fs::write(&file, "demo\n").unwrap();

    let mut ipath_pattern = root.path().as_os_str().as_bytes().to_vec();
    ipath_pattern.extend_from_slice(b"/readme-\xff.txt");

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            os(b"ReadMe-\xff.TXT"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-iname".into(),
            os(b"readme-\xff.txt"),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-path".into(),
            file.as_os_str().to_os_string(),
            "-print0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-ipath".into(),
            os(&ipath_pattern),
            "-print0".into(),
        ],
    ] {
        let output = cargo_bin_output_with_timeout(&args, 1, Duration::from_secs(5));
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            nul_records(&output.stdout),
            std::iter::once(path_bytes(&file)).collect()
        );
    }
}

#[test]
fn lname_and_ilname_accept_non_utf8_targets() {
    let root = tempdir().unwrap();
    let target = path_from_bytes(b"TarGet-\xfe.bin");
    let link = root.path().join("link");
    unix_fs::symlink(&target, &link).unwrap();

    for (flag, pattern) in [
        ("-lname", os(b"TarGet-\xfe.bin")),
        ("-ilname", os(b"target-\xfe.bin")),
    ] {
        let output = cargo_bin_output_with_timeout(
            &[
                path_arg(root.path()),
                "-maxdepth".into(),
                "1".into(),
                flag.into(),
                pattern,
                "-print0".into(),
            ],
            1,
            Duration::from_secs(5),
        );
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            nul_records(&output.stdout),
            std::iter::once(path_bytes(&link)).collect()
        );
    }
}

#[test]
fn printf_and_fprintf_preserve_non_utf8_path_and_link_target_bytes() {
    let root = tempdir().unwrap();
    let link = root.path().join(path_from_bytes(b"sym-\xfd"));
    let out = root.path().join("report.txt");
    unix_fs::symlink(path_from_bytes(b"TarGet-\xfe.bin"), &link).unwrap();

    let link_bytes = path_bytes(&link);
    let root_bytes = path_bytes(root.path());
    let expected = [
        b"[".as_slice(),
        link_bytes.as_slice(),
        b"][".as_slice(),
        b"sym-\xfd".as_slice(),
        b"][".as_slice(),
        root_bytes.as_slice(),
        b"][".as_slice(),
        b"sym-\xfd".as_slice(),
        b"][".as_slice(),
        root_bytes.as_slice(),
        b"][TarGet-\xfe.bin]\n".as_slice(),
    ]
    .concat();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-type".into(),
            "l".into(),
            "-printf".into(),
            "[%p][%P][%H][%f][%h][%l]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected);

    let file_output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-type".into(),
            "l".into(),
            "-fprintf".into(),
            path_arg(&out),
            "[%p][%P][%H][%f][%h][%l]\\n".into(),
        ],
        1,
        Duration::from_secs(5),
    );
    assert_eq!(file_output.status.code(), Some(0));
    assert_eq!(fs::read(&out).unwrap(), expected);
}
