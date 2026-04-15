mod support;

use assert_cmd::cargo::CommandCargoExt;
use findoxide::birth::read_birth_time;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{self as unix_fs, MetadataExt, PermissionsExt};
use std::path::Path;
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

fn build_perm_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file-664"), "hello\n").unwrap();
    fs::write(root.path().join("file-660"), "hello\n").unwrap();
    fs::write(root.path().join("file-000"), "hello\n").unwrap();
    fs::write(root.path().join("file-sticky"), "hello\n").unwrap();
    fs::set_permissions(
        root.path().join("file-664"),
        fs::Permissions::from_mode(0o664),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("file-660"),
        fs::Permissions::from_mode(0o660),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("file-000"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("file-sticky"),
        fs::Permissions::from_mode(0o1000),
    )
    .unwrap();
    root
}

fn build_size_time_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join("empty.bin"), []).unwrap();
    fs::write(root.path().join("blockish.bin"), vec![b'x'; 513]).unwrap();
    fs::write(root.path().join("large.bin"), vec![b'x'; 2049]).unwrap();
    fs::write(root.path().join("older.txt"), "older\n").unwrap();
    fs::write(root.path().join("recent.txt"), "recent\n").unwrap();
    fs::write(root.path().join("reference.txt"), "reference\n").unwrap();
    unix_fs::symlink("reference.txt", root.path().join("reference-link")).unwrap();

    touch_time(&root.path().join("older.txt"), &["-a", "-d", "2 days ago"]);
    touch_time(&root.path().join("older.txt"), &["-m", "-d", "2 days ago"]);
    touch_time(
        &root.path().join("recent.txt"),
        &["-a", "-d", "15 minutes ago"],
    );
    touch_time(
        &root.path().join("recent.txt"),
        &["-m", "-d", "15 minutes ago"],
    );
    touch_time(
        &root.path().join("reference.txt"),
        &["-a", "-d", "3 days ago"],
    );
    touch_time(
        &root.path().join("reference.txt"),
        &["-m", "-d", "1 day ago"],
    );

    root
}

fn build_read_only_tail_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("empty-dir")).unwrap();
    fs::create_dir(root.path().join("nonempty-dir")).unwrap();
    fs::write(root.path().join("nonempty-dir/child"), "child\n").unwrap();
    fs::write(root.path().join("empty-file"), []).unwrap();
    fs::write(root.path().join("nonempty-file"), "hello\n").unwrap();
    fs::write(root.path().join("reference-file"), "reference\n").unwrap();
    fs::write(root.path().join("used-one.txt"), "one\n").unwrap();
    fs::write(root.path().join("used-negative.txt"), "negative\n").unwrap();
    unix_fs::symlink("empty-dir", root.path().join("empty-dir-link")).unwrap();
    unix_fs::symlink("reference-file", root.path().join("reference-link")).unwrap();

    touch_time(
        &root.path().join("used-one.txt"),
        &["-a", "-m", "-d", "2 days ago"],
    );
    touch_time(
        &root.path().join("used-negative.txt"),
        &["-a", "-m", "-d", "2 days ago"],
    );
    toggle_user_execute(&root.path().join("used-one.txt"));
    toggle_user_execute(&root.path().join("used-negative.txt"));
    let _ = fs::read(root.path().join("used-one.txt")).unwrap();

    root
}

fn touch_time(path: &Path, args: &[&str]) {
    let status = Command::new("touch").args(args).arg(path).status().unwrap();
    assert!(status.success(), "touch failed for {}", path.display());
}

fn toggle_user_execute(path: &Path) {
    let metadata = fs::metadata(path).unwrap();
    let mut permissions = metadata.permissions();
    permissions.set_mode(metadata.permissions().mode() ^ 0o100);
    fs::set_permissions(path, permissions).unwrap();
}

fn gnu_supports_birth_time_predicates(root: &Path) -> bool {
    Command::new("find")
        .arg(root)
        .args(["-newerBt", "@1700000000.25"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn current_id_output(flag: &str) -> String {
    let output = Command::new("id").arg(flag).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn assert_matches_gnu_exact(args: &[OsString]) {
    let expected = Command::new("find").args(args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .args(args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

fn assert_matches_gnu_as_sets(args: &[OsString]) {
    let expected = Command::new("find").args(args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
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
    assert!(readme.contains("`-uid`"));
    assert!(readme.contains("`-gid`"));
    assert!(readme.contains("`-user`"));
    assert!(readme.contains("`-group`"));
    assert!(readme.contains("`-nouser`"));
    assert!(readme.contains("`-nogroup`"));
    assert!(readme.contains("`-perm`"));
    assert!(readme.contains("lazy entry data access"));
    assert!(readme.contains("cheap-first planning"));
    assert!(readme.contains("loop-safe"));
}

#[test]
fn readme_documents_stage9_read_only_tail_surface() {
    let readme = fs::read_to_string("README.md").unwrap();

    assert!(readme.contains("`-size`"));
    assert!(readme.contains("`-empty`"));
    assert!(readme.contains("`-used`"));
    assert!(readme.contains("`-mtime`"));
    assert!(readme.contains("`-atime`"));
    assert!(readme.contains("`-ctime`"));
    assert!(readme.contains("`-mmin`"));
    assert!(readme.contains("`-amin`"));
    assert!(readme.contains("`-cmin`"));
    assert!(readme.contains("`-newer`"));
    assert!(readme.contains("`-anewer`"));
    assert!(readme.contains("`-cnewer`"));
    assert!(readme.contains("full read-only `-newerXY`"));
    assert!(readme.contains("`-daystart`"));
    assert!(readme.contains("`@<unix-seconds>[.frac]`"));
    assert!(readme.contains("`YYYY-MM-DD`"));
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
fn ordered_metadata_ownership_matches_gnu_find_exactly() {
    let root = build_tree();
    let metadata = fs::metadata(root.path().join("src/lib.rs")).unwrap();
    let uid = metadata.uid().to_string();
    let gid = metadata.gid().to_string();
    let user = current_id_output("-un");
    let group = current_id_output("-gn");
    let args_sets = vec![
        vec![path_arg(root.path()), "-uid".into(), uid.clone().into()],
        vec![path_arg(root.path()), "-gid".into(), gid.clone().into()],
        vec![path_arg(root.path()), "-user".into(), user.into()],
        vec![path_arg(root.path()), "-group".into(), group.into()],
        vec![path_arg(root.path()), "-nouser".into()],
        vec![path_arg(root.path()), "-nogroup".into()],
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
fn parallel_metadata_ownership_matches_gnu_find_as_sets() {
    let root = build_tree();
    let metadata = fs::metadata(root.path().join("src/lib.rs")).unwrap();
    let uid = metadata.uid().to_string();
    let gid = metadata.gid().to_string();
    let user = current_id_output("-un");
    let group = current_id_output("-gn");
    let args_sets = vec![
        vec![path_arg(root.path()), "-uid".into(), uid.into()],
        vec![path_arg(root.path()), "-gid".into(), gid.into()],
        vec![path_arg(root.path()), "-user".into(), user.into()],
        vec![path_arg(root.path()), "-group".into(), group.into()],
        vec![path_arg(root.path()), "-nouser".into()],
        vec![path_arg(root.path()), "-nogroup".into()],
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

#[test]
fn ordered_perm_matches_gnu_find_exactly() {
    let root = build_perm_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-perm".into(), "664".into()],
        vec![path_arg(root.path()), "-perm".into(), "-g+w,u+w".into()],
        vec![path_arg(root.path()), "-perm".into(), "/u=w,g=w".into()],
        vec![path_arg(root.path()), "-perm".into(), "g=u".into()],
        vec![path_arg(root.path()), "-perm".into(), "u=".into()],
        vec![path_arg(root.path()), "-perm".into(), "-g=u".into()],
        vec![path_arg(root.path()), "-perm".into(), "-u=X".into()],
        vec![path_arg(root.path()), "-perm".into(), "+t".into()],
        vec![path_arg(root.path()), "-perm".into(), "+X".into()],
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
fn parallel_perm_matches_gnu_find_as_sets() {
    let root = build_perm_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-perm".into(), "-g+w,u+w".into()],
        vec![path_arg(root.path()), "-perm".into(), "/u=w,g=w".into()],
        vec![path_arg(root.path()), "-perm".into(), "g=u".into()],
        vec![path_arg(root.path()), "-perm".into(), "u=".into()],
        vec![path_arg(root.path()), "-perm".into(), "-g=u".into()],
        vec![path_arg(root.path()), "-perm".into(), "-u=X".into()],
        vec![path_arg(root.path()), "-perm".into(), "+t".into()],
        vec![path_arg(root.path()), "-perm".into(), "+X".into()],
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

#[test]
fn ordered_stage8_predicates_match_gnu_find_exactly() {
    let root = build_size_time_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-size".into(), "2b".into()],
        vec![path_arg(root.path()), "-size".into(), "-1M".into()],
        vec![path_arg(root.path()), "-mtime".into(), "+1".into()],
        vec![path_arg(root.path()), "-atime".into(), "+1".into()],
        vec![path_arg(root.path()), "-ctime".into(), "0".into()],
        vec![path_arg(root.path()), "-mmin".into(), "+5".into()],
        vec![path_arg(root.path()), "-amin".into(), "+5".into()],
        vec![path_arg(root.path()), "-cmin".into(), "-1".into()],
        vec![
            path_arg(root.path()),
            "-daystart".into(),
            "-mtime".into(),
            "0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newer".into(),
            path_arg(&root.path().join("reference.txt")),
        ],
        vec![
            path_arg(root.path()),
            "-anewer".into(),
            path_arg(&root.path().join("reference.txt")),
        ],
        vec![
            path_arg(root.path()),
            "-cnewer".into(),
            path_arg(&root.path().join("reference.txt")),
        ],
        vec![
            path_arg(root.path()),
            "-newerma".into(),
            path_arg(&root.path().join("reference.txt")),
        ],
        vec![
            "-P".into(),
            path_arg(root.path()),
            "-newer".into(),
            path_arg(&root.path().join("reference-link")),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-newer".into(),
            path_arg(&root.path().join("reference-link")),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn parallel_stage8_predicates_match_gnu_find_as_a_set() {
    let root = build_size_time_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "(".into(),
            "-size".into(),
            "+1c".into(),
            "-a".into(),
            "-daystart".into(),
            "-mtime".into(),
            "0".into(),
            ")".into(),
            "-o".into(),
            "(".into(),
            "-newer".into(),
            path_arg(&root.path().join("reference.txt")),
            "-a".into(),
            "-type".into(),
            "f".into(),
            ")".into(),
        ],
        vec![
            path_arg(root.path()),
            "(".into(),
            "-amin".into(),
            "+5".into(),
            "-a".into(),
            "-size".into(),
            "+0c".into(),
            ")".into(),
            "-o".into(),
            "(".into(),
            "-newerma".into(),
            path_arg(&root.path().join("reference.txt")),
            "-a".into(),
            "-type".into(),
            "f".into(),
            ")".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_as_sets(&args);
    }
}

#[test]
fn ordered_stage9_read_only_tail_matches_gnu_find_exactly() {
    let root = build_read_only_tail_tree();
    let mut args_sets = vec![
        vec![path_arg(root.path()), "-empty".into()],
        vec![path_arg(root.path()), "-used".into(), "0".into()],
        vec![path_arg(root.path()), "-used".into(), "1".into()],
        vec![path_arg(root.path()), "-used".into(), "-1".into()],
        vec!["-L".into(), path_arg(root.path()), "-empty".into()],
    ];
    let gnu_supports_birth = gnu_supports_birth_time_predicates(root.path());

    if gnu_supports_birth {
        args_sets.push(vec![
            path_arg(root.path()),
            "-newerBt".into(),
            "@1700000000.25".into(),
        ]);
    }

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }

    if gnu_supports_birth
        && read_birth_time(&root.path().join("reference-file"), true)
            .unwrap()
            .is_some()
    {
        assert_matches_gnu_exact(&[
            path_arg(root.path()),
            "-newermB".into(),
            path_arg(&root.path().join("reference-file")),
        ]);
    }
}

#[test]
fn parallel_stage9_read_only_tail_matches_gnu_find_as_sets() {
    let root = build_read_only_tail_tree();
    let args = vec![
        path_arg(root.path()),
        "-mindepth".into(),
        "1".into(),
        "(".into(),
        "-empty".into(),
        "-o".into(),
        "-used".into(),
        "1".into(),
        "-o".into(),
        "-used".into(),
        "-1".into(),
        ")".into(),
        "-a".into(),
        "(".into(),
        "-type".into(),
        "f".into(),
        "-o".into(),
        "-type".into(),
        "d".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}
