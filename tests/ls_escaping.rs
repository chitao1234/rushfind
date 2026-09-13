#![cfg(unix)]
//! `find -ls` escapes the printed name one byte at a time. These tests pin the
//! bytes rfd writes and, when a GNU `find` is on `PATH`, compare them to it.

mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs as unix_fs;
use std::path::Path;
use std::time::Duration;
use support::{
    assert_file_output_matches_gnu_with_env, assert_matches_gnu_exact_with_env,
    cargo_bin_output_with_timeout, newline_records, path_arg, supports_non_utf8_temp_paths,
};
use tempfile::tempdir;

fn os(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

/// One name per branch of GNU's rule: the named C escapes, the two characters
/// GNU backslashes, the printable bytes that must survive verbatim, and names
/// that only look dangerous.
const NAMES: &[&[u8]] = &[
    b"plain.txt",
    b"has space",
    b"has\ttab",
    b"has\nnewline",
    b"has\rcr",
    b"has\x08backspace",
    b"has\x0cformfeed",
    b"has\x0bverticaltab",
    b"has\\backslash",
    b"has\"dquote",
    b"has'squote",
    b"has\x01ctrl",
    b"has\x7fdel",
    b"has\x1besc",
    b"\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e.txt",
    b"caf\xc3\xa9",
    b"star*name",
    b"quest?name",
    b"brack[name",
    b"-dashfile",
    b"tilde~dollar$pipe|semi;",
];

/// `(link name, target)` pairs; both sides go through the same escaping.
const SYMLINKS: &[(&[u8], &[u8])] = &[
    (b"link ctrl", b"tgt\x01ctrl"),
    (b"link space", b"tgt with space"),
    (b"link quote", b"tgt\"dquote"),
    (b"link backslash", b"tgt\\backslash"),
    (b"link utf8", b"\xc3\xa9utf8"),
    (b"link newline", b"tgt\nnl"),
];

fn build_matrix(root: &Path) {
    for name in NAMES {
        fs::write(root.join(os(name)), b"x").unwrap();
    }
    for (name, target) in SYMLINKS {
        unix_fs::symlink(os(target), root.join(os(name))).unwrap();
    }
}

/// The record tail a start path of `root` produces for the escaped name.
fn name_field(root: &Path, escaped: &[u8]) -> Vec<u8> {
    let mut path = root.as_os_str().as_bytes().to_vec();
    path.push(b'/');
    path.extend_from_slice(escaped);
    path
}

fn ls_records(root: &Path, max_depth: u32) -> Vec<Vec<u8>> {
    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root),
            "-maxdepth".into(),
            max_depth.to_string().into(),
            "-ls".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    newline_records(&output.stdout).into_iter().collect()
}

fn assert_record_ending(root: &Path, records: &[Vec<u8>], escaped: &[u8]) {
    let wanted = name_field(root, escaped);
    assert!(
        records.iter().any(|record| record.ends_with(&wanted)),
        "no record ending in {:?}:\n{}",
        String::from_utf8_lossy(&wanted),
        records
            .iter()
            .map(|record| String::from_utf8_lossy(record).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// No byte a terminal would interpret as a control character may reach the
/// output: after escaping, a record is printable ASCII.
fn assert_no_raw_bytes(records: &[Vec<u8>]) {
    for record in records {
        for byte in record {
            assert!(
                (0x20..=0x7e).contains(byte),
                "record {:?} kept a raw byte {byte:#04x}",
                String::from_utf8_lossy(record)
            );
        }
    }
}

/// `-ls` writes the C escapes it shares with `-printf` (`\t`, `\n`, `\r`,
/// `\b`, `\f`) and `\\` for a backslash, but only `\NNN` for the rest of the
/// control bytes; a space and a double quote get a backslash, and printable
/// ASCII is left alone.
#[test]
fn ls_record_names_are_escaped_byte_for_byte() {
    let root = tempdir().unwrap();
    build_matrix(root.path());
    let records = ls_records(root.path(), 1);

    let expected: &[&[u8]] = &[
        b"plain.txt",
        b"has\\ space",
        b"has\\ttab",
        b"has\\nnewline",
        b"has\\rcr",
        b"has\\bbackspace",
        b"has\\fformfeed",
        b"has\\013verticaltab",
        b"has\\\\backslash",
        b"has\\\"dquote",
        b"has'squote",
        b"has\\001ctrl",
        b"has\\177del",
        b"has\\033esc",
        b"\\346\\227\\245\\346\\234\\254\\350\\252\\236.txt",
        b"caf\\303\\251",
        b"star*name",
        b"quest?name",
        b"brack[name",
        b"-dashfile",
        b"tilde~dollar$pipe|semi;",
    ];
    for name in expected {
        assert_record_ending(root.path(), &records, name);
    }
    assert_no_raw_bytes(&records);
}

#[test]
fn ls_escapes_symlink_targets_with_the_same_rule() {
    let root = tempdir().unwrap();
    build_matrix(root.path());
    let records = ls_records(root.path(), 1);

    let expected: &[&[u8]] = &[
        b"link\\ ctrl -> tgt\\001ctrl",
        b"link\\ space -> tgt\\ with\\ space",
        b"link\\ quote -> tgt\\\"dquote",
        b"link\\ backslash -> tgt\\\\backslash",
        b"link\\ utf8 -> \\303\\251utf8",
        b"link\\ newline -> tgt\\nnl",
    ];
    for suffix in expected {
        assert_record_ending(root.path(), &records, suffix);
    }
    assert_no_raw_bytes(&records);
}

#[test]
fn ls_escapes_names_nested_below_a_start_path() {
    let root = tempdir().unwrap();
    let dir = root.path().join("dir with space");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join(os(b"we\x01ird")), b"x").unwrap();
    fs::write(dir.join("plain"), b"x").unwrap();

    let records = ls_records(root.path(), 2);
    assert_record_ending(root.path(), &records, b"dir\\ with\\ space");
    assert_record_ending(root.path(), &records, b"dir\\ with\\ space/plain");
    assert_record_ending(root.path(), &records, b"dir\\ with\\ space/we\\001ird");
}

#[test]
fn ls_escapes_every_byte_like_gnu_find() {
    let root = tempdir().unwrap();
    build_matrix(root.path());

    assert_matches_gnu_exact_with_env(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-ls".into(),
    ]);
}

#[test]
fn fls_escapes_every_byte_like_gnu_find() {
    let root = tempdir().unwrap();
    build_matrix(root.path());

    assert_file_output_matches_gnu_with_env(
        &[path_arg(root.path()), "-maxdepth".into(), "1".into()],
        "-fls",
        1,
        "escaping.ls",
        &[],
    );
}

/// Raw non-UTF-8 bytes are one more `\NNN` case. Not every filesystem can hold
/// them (macOS rejects them with EILSEQ), so this test self-skips there.
#[test]
fn ls_escapes_raw_non_utf8_bytes_as_octal() {
    if !supports_non_utf8_temp_paths() {
        return;
    }

    let root = tempdir().unwrap();
    fs::write(root.path().join(os(b"we\x01ird\xff")), b"x").unwrap();
    fs::write(root.path().join(os(b"\x80\xfe")), b"x").unwrap();

    let records = ls_records(root.path(), 1);
    assert_record_ending(root.path(), &records, b"we\\001ird\\377");
    assert_record_ending(root.path(), &records, b"\\200\\376");
    assert_no_raw_bytes(&records);

    assert_matches_gnu_exact_with_env(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-ls".into(),
    ]);
}
