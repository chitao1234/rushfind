#![cfg(any(target_os = "solaris", target_os = "illumos"))]

use crate::support::{cargo_bin_output_with_timeout, ensure_gnu_find, gnu_find_command, path_arg};
use rushfind::entry::{EntryContext, EntryKind};
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

unsafe extern "C" {
    fn rushfind_test_create_door(path: *const libc::c_char) -> libc::c_int;
    fn rushfind_test_destroy_door(fd: libc::c_int, path: *const libc::c_char) -> libc::c_int;
}

struct AttachedDoor {
    fd: libc::c_int,
    path: std::ffi::CString,
}

impl AttachedDoor {
    fn new(path: &Path) -> Self {
        let path_c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let fd = unsafe { rushfind_test_create_door(path_c.as_ptr()) };
        assert!(fd >= 0, "door_create: {}", std::io::Error::last_os_error());
        Self { fd, path: path_c }
    }
}

impl Drop for AttachedDoor {
    fn drop(&mut self) {
        let result = unsafe { rushfind_test_destroy_door(self.fd, self.path.as_ptr()) };
        if !std::thread::panicking() {
            assert_eq!(result, 0, "could not destroy door fixture");
        }
    }
}

fn run(args: &[OsString], workers: usize) -> Vec<u8> {
    let output = cargo_bin_output_with_timeout(args, workers, Duration::from_secs(5));
    assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
    assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    output.stdout
}

fn assert_matches(root: &Path, follow: &str, flag: &str, kinds: &str, expected: &[&str]) {
    let args = [
        follow.into(),
        path_arg(root),
        flag.into(),
        kinds.into(),
        "-printf".into(),
        "%f\n".into(),
    ];
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    for workers in [1, 4] {
        let bytes = run(&args, workers);
        let text = String::from_utf8(bytes).unwrap();
        let mut names = text.lines().collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, expected, "workers={workers} args={args:?}");
    }
}

#[test]
fn door_metadata_and_directory_hints_match_real_attached_doors() {
    let root = tempdir().unwrap();
    let path = root.path().join("service.door");
    let door = AttachedDoor::new(&path);
    fs::write(root.path().join("regular"), b"").unwrap();
    symlink("service.door", root.path().join("link")).unwrap();

    assert_eq!(fs::metadata(&path).unwrap().mode() & 0xF000, 0xD000);
    let entry = EntryContext::new(path.clone(), 0, true);
    assert_eq!(entry.physical_kind().unwrap(), EntryKind::Door);
    assert_matches(&path, "-P", "-type", "D", &["service.door"]);
    assert_matches(&path, "-P", "-type", "f", &[]);
    assert_matches(root.path(), "-P", "-type", "D", &["service.door"]);
    assert_matches(
        root.path(),
        "-P",
        "-type",
        "f,D",
        &["regular", "service.door"],
    );
    assert_matches(root.path(), "-P", "-type", "f", &["regular"]);
    let empty = run(&[path_arg(&path), "-empty".into(), "-print".into()], 1);
    assert!(empty.is_empty());

    drop(door);
    // Detaching reveals the regular backing file. No success is possible by
    // treating every unusual entry as a door or by matching D unconditionally.
    assert_matches(&path, "-P", "-type", "D", &[]);
    assert_matches(&path, "-P", "-type", "f", &["service.door"]);
}

#[test]
fn door_symlinks_obey_physical_command_line_and_logical_follow_modes() {
    let root = tempdir().unwrap();
    let _door = AttachedDoor::new(&root.path().join("service.door"));
    let link = root.path().join("link");
    symlink("service.door", &link).unwrap();
    for follow in ["-P", "-H"] {
        assert_matches(root.path(), follow, "-type", "D", &["service.door"]);
        assert_matches(
            root.path(),
            follow,
            "-xtype",
            "D",
            &["link", "service.door"],
        );
    }
    assert_matches(root.path(), "-L", "-type", "D", &["link", "service.door"]);
    assert_matches(root.path(), "-L", "-xtype", "D", &["service.door"]);
    assert_matches(&link, "-P", "-type", "D", &[]);
    assert_matches(&link, "-P", "-xtype", "D", &["link"]);
    assert_matches(&link, "-H", "-type", "D", &["link"]);
    assert_matches(&link, "-H", "-xtype", "D", &["link"]);
    assert_matches(&link, "-H", "-xtype", "l", &[]);
    for follow in ["-L"] {
        assert_matches(&link, follow, "-type", "D", &["link"]);
        assert_matches(&link, follow, "-xtype", "D", &[]);
        assert_matches(&link, follow, "-xtype", "l", &["link"]);
    }
}

#[test]
fn door_printf_and_ls_render_native_type_letters() {
    let root = tempdir().unwrap();
    let path = root.path().join("service.door");
    let _door = AttachedDoor::new(&path);
    let link = root.path().join("link");
    symlink("service.door", &link).unwrap();
    let format = "%y|%Y|%M\n";
    let args = [path_arg(&path), "-printf".into(), format.into()];
    for workers in [1, 4] {
        assert_eq!(run(&args, workers), b"D|D|Drw-------\n");
        let args = [path_arg(&link), "-printf".into(), "%y|%Y\n".into()];
        assert_eq!(run(&args, workers), b"l|D\n");
        let args = [
            "-L".into(),
            path_arg(&link),
            "-printf".into(),
            format.into(),
        ];
        assert_eq!(run(&args, workers), b"D|D|Drw-------\n");
        let output = run(&[path_arg(&path), "-ls".into()], workers);
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.split_whitespace().nth(2), Some("Drw-------"));
        let dest = root.path().join("listing");
        let output = run(&[path_arg(&path), "-fls".into(), path_arg(&dest)], workers);
        assert!(output.is_empty());
        assert_eq!(
            fs::read_to_string(&dest).unwrap().split_whitespace().nth(2),
            Some("Drw-------")
        );
    }
}

#[test]
fn native_door_matches_gnu_find_for_types_follow_modes_and_rendering() {
    let root = tempdir().unwrap();
    let _door = AttachedDoor::new(&root.path().join("service.door"));
    symlink("service.door", root.path().join("link")).unwrap();
    fs::write(root.path().join("regular"), b"").unwrap();
    if !ensure_gnu_find() {
        return;
    }
    for follow in ["-P", "-H", "-L"] {
        for flag in ["-type", "-xtype"] {
            for kinds in ["D", "f,D", "l,D"] {
                let args = [
                    follow.into(),
                    path_arg(root.path()),
                    flag.into(),
                    kinds.into(),
                    "-printf".into(),
                    "%f|%y|%Y|%M\n".into(),
                ];
                let expected = gnu_find_command().unwrap().args(&args).output().unwrap();
                assert!(expected.status.success(), "{args:?}: {expected:?}");
                assert!(expected.stderr.is_empty(), "{args:?}: {expected:?}");
                for workers in [1, 4] {
                    let mut actual = run(&args, workers)
                        .split(|b| *b == b'\n')
                        .map(<[u8]>::to_vec)
                        .collect::<Vec<_>>();
                    let mut expected = expected
                        .stdout
                        .split(|b| *b == b'\n')
                        .map(<[u8]>::to_vec)
                        .collect::<Vec<_>>();
                    actual.sort();
                    expected.sort();
                    assert_eq!(actual, expected, "workers={workers} {args:?}");
                }
            }
        }
    }
}
