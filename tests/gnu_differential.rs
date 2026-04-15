mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::os::unix::fs::{self as unix_fs, MetadataExt};
use std::process::Command;
use support::{lines, path_arg};
use tempfile::tempdir;

fn build_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.path().join("docs/spec.md"), "# spec\n").unwrap();
    root
}

fn build_identity_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "hello\n").unwrap();
    fs::hard_link(
        root.path().join("real/file.txt"),
        root.path().join("real/file-hard.txt"),
    )
    .unwrap();
    unix_fs::symlink(
        root.path().join("real/file.txt"),
        root.path().join("file-link"),
    )
    .unwrap();
    unix_fs::symlink(root.path().join("missing"), root.path().join("broken-link")).unwrap();
    root
}

fn build_symlink_content_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(
        root.path().join("real/file.txt"),
        root.path().join("file-link"),
    )
    .unwrap();
    unix_fs::symlink("missing-target", root.path().join("broken-link")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("root-link")).unwrap();
    unix_fs::symlink("missing-target", root.path().join("broken-root")).unwrap();
    unix_fs::symlink("file.txt", root.path().join("real/child-link")).unwrap();
    root
}

#[test]
fn readme_documents_worker_selection_contract() {
    let readme = fs::read_to_string("README.md").unwrap();

    assert!(readme.contains("FINDOXIDE_WORKERS"));
    assert!(readme.contains("GNU `find` syntax"));
    assert!(readme.contains("`-P`, `-H`, `-L`"));
    assert!(readme.contains("`-xtype`"));
    assert!(readme.contains("`-samefile`"));
    assert!(readme.contains("`-inum`"));
    assert!(readme.contains("`-links`"));
    assert!(readme.contains("`-lname`"));
    assert!(readme.contains("`-ilname`"));
    assert!(readme.contains("loop-safe"));
}

#[test]
fn reports_unsupported_exec_during_planning() {
    let root = build_tree();
    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .arg(root.path())
        .args(["-exec", "echo", "{}", ";"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unsupported in read-only v0"));
}

#[test]
fn reports_parse_errors_nonzero() {
    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .args(["(", "-name", "*.rs"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("expected `)`"));
}

#[test]
fn ordered_mode_matches_gnu_find_exactly() {
    let root = build_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-name".into(),
        "*.rs".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
}

#[test]
fn parallel_mode_matches_gnu_find_as_a_set() {
    let root = build_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-name".into(),
        "*.rs".into(),
        "-o".into(),
        "-name".into(),
        "*.md".into(),
        ")".into(),
        "-type".into(),
        "f".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}

#[test]
fn ordered_follow_modes_match_gnu_find_exactly() {
    let root = build_tree();
    unix_fs::symlink(root.path().join("src"), root.path().join("src-link")).unwrap();

    for args in [
        vec![
            "-P".into(),
            path_arg(root.path()),
            "-type".into(),
            "l".into(),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-xtype".into(),
            "l".into(),
        ],
        vec![
            "-H".into(),
            path_arg(root.path().join("src-link").as_path()),
            "-type".into(),
            "d".into(),
        ],
    ] {
        let expected = Command::new("find").args(&args).output().unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "1")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
}

#[test]
fn ordered_alias_preservation_matches_gnu_find_exactly() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::create_dir(root.path().join("real/sub")).unwrap();
    fs::write(root.path().join("real/sub/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-a")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-b")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "file.txt".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn parallel_follow_modes_match_gnu_find_as_sets() {
    let root = build_tree();
    unix_fs::symlink(root.path().join("src"), root.path().join("src-link")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "(".into(),
        "-name".into(),
        "*.rs".into(),
        "-o".into(),
        "-xtype".into(),
        "l".into(),
        ")".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}

#[test]
fn parallel_alias_preservation_matches_gnu_find_as_sets() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::create_dir(root.path().join("real/sub")).unwrap();
    fs::write(root.path().join("real/sub/file.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-a")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("link-b")).unwrap();

    let args = vec![
        "-L".into(),
        path_arg(root.path()),
        "-name".into(),
        "file.txt".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}

#[test]
fn ordered_family_a_matches_gnu_find_exactly() {
    let root = build_identity_tree();
    let logical_inode = fs::metadata(root.path().join("file-link"))
        .unwrap()
        .ino()
        .to_string();
    let args_sets = vec![
        vec![
            "-P".into(),
            path_arg(root.path()),
            "-samefile".into(),
            path_arg(&root.path().join("file-link")),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-samefile".into(),
            path_arg(&root.path().join("file-link")),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-inum".into(),
            logical_inode.clone().into(),
        ],
        vec![path_arg(root.path()), "-links".into(), "2".into()],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-samefile".into(),
            path_arg(&root.path().join("broken-link")),
        ],
    ];

    for args in args_sets {
        let expected = Command::new("find").args(&args).output().unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "1")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
}

#[test]
fn parallel_family_a_matches_gnu_find_as_sets() {
    let root = build_identity_tree();
    let logical_inode = fs::metadata(root.path().join("file-link"))
        .unwrap()
        .ino()
        .to_string();
    let args_sets = vec![
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-samefile".into(),
            path_arg(&root.path().join("file-link")),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-inum".into(),
            logical_inode.into(),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-links".into(),
            "2".into(),
        ],
    ];

    for args in args_sets {
        let expected = Command::new("find").args(&args).output().unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "4")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    }
}

#[test]
fn ordered_symlink_content_matches_gnu_find_exactly() {
    let root = build_symlink_content_tree();
    let args_sets = vec![
        vec![
            "-P".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-H".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-H".into(),
            path_arg(&root.path().join("root-link")),
            "-lname".into(),
            "*file.txt".into(),
        ],
        vec![
            "-H".into(),
            path_arg(&root.path().join("broken-root")),
            "-ilname".into(),
            "*MISSING*".into(),
        ],
    ];

    for args in args_sets {
        let expected = Command::new("find").args(&args).output().unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "1")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
}

#[test]
fn parallel_symlink_content_matches_gnu_find_as_sets() {
    let root = build_symlink_content_tree();
    let args_sets = vec![
        vec![
            "-P".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-H".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "(".into(),
            "-lname".into(),
            "*file.txt".into(),
            "-o".into(),
            "-ilname".into(),
            "*MISSING*".into(),
            ")".into(),
        ],
        vec![
            "-H".into(),
            path_arg(&root.path().join("root-link")),
            "-lname".into(),
            "*file.txt".into(),
        ],
        vec![
            "-H".into(),
            path_arg(&root.path().join("broken-root")),
            "-ilname".into(),
            "*MISSING*".into(),
        ],
    ];

    for args in args_sets {
        let expected = Command::new("find").args(&args).output().unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "4")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
        assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
    }
}
