mod support;

use assert_cmd::cargo::CommandCargoExt;
use findoxide::birth::read_birth_time;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{self as unix_fs, MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;
use support::{
    PRINTF_TIME_TZ, assert_file_output_matches_gnu_with_env, assert_matches_gnu_as_sets,
    assert_matches_gnu_as_sets_with_env, assert_matches_gnu_exact,
    assert_matches_gnu_exact_with_env, assert_matches_gnu_exact_with_input,
    assert_matches_gnu_regex_outcome, assert_matches_gnu_regex_outcome_as_sets, lines,
    normalize_warning_program, path_arg,
};
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

fn build_prune_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("vendor")).unwrap();
    fs::create_dir(root.path().join("vendor/nested")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(
        root.path().join("vendor/blocked.rs"),
        "pub fn blocked() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("vendor/nested/deeper.rs"),
        "pub fn deeper() {}\n",
    )
    .unwrap();
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

fn build_access_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join("readable.txt"), "readable\n").unwrap();
    fs::write(root.path().join("locked.txt"), "locked\n").unwrap();
    fs::write(root.path().join("script.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    fs::create_dir(root.path().join("searchable-dir")).unwrap();
    fs::write(root.path().join("searchable-dir/child.txt"), "child\n").unwrap();
    unix_fs::symlink("readable.txt", root.path().join("readable-link")).unwrap();
    unix_fs::symlink("missing-target", root.path().join("broken-link")).unwrap();
    unix_fs::symlink("readable.txt", root.path().join("root-link")).unwrap();

    fs::set_permissions(
        root.path().join("locked.txt"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("script.sh"),
        fs::Permissions::from_mode(0o755),
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

fn build_exec_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "alpha\n").unwrap();
    fs::write(root.path().join("beta.txt"), "beta\n").unwrap();
    root
}

fn build_execdir_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::create_dir(root.path().join("start")).unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("dir/alpha.txt"), "alpha\n").unwrap();
    fs::write(root.path().join("dir/beta.txt"), "beta\n").unwrap();
    fs::write(root.path().join("real/file.txt"), "file\n").unwrap();
    unix_fs::symlink("../real/file.txt", root.path().join("start/link")).unwrap();
    root
}

fn build_printf_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "hello").unwrap();
    fs::set_permissions(
        root.path().join("dir/file.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    fs::hard_link(
        root.path().join("dir/file.txt"),
        root.path().join("dir/file-hard.txt"),
    )
    .unwrap();
    unix_fs::symlink("dir/file.txt", root.path().join("link.txt")).unwrap();
    let sparse = fs::File::create(root.path().join("nested/sparse.bin")).unwrap();
    sparse.set_len(8192).unwrap();
    root
}

fn build_printf_target_type_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("file.txt"), "hello").unwrap();
    unix_fs::symlink("file.txt", root.path().join("file-link")).unwrap();
    unix_fs::symlink("dir", root.path().join("dir-link")).unwrap();
    unix_fs::symlink("missing", root.path().join("missing-link")).unwrap();
    unix_fs::symlink("loop", root.path().join("loop")).unwrap();
    root
}

fn build_printf_sparseness_tree() -> tempfile::TempDir {
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
    root
}

fn build_regex_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.path().join("README.MD"), "# readme\n").unwrap();
    fs::write(root.path().join("docs/Guide.txt"), "guide\n").unwrap();
    root
}

fn build_regex_extension_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in ["paren)", "+foo", "?foo", "aa", "ab", "foo", "foobar"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

fn build_emacs_regex_edge_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in ["a", "aa", "a+", "a^b", "ab", "]", "\\]"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

fn build_bre_interval_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in ["b", "ab", "aab", "aaab", "aaaab", "c"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

fn build_delete_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("tree")).unwrap();
    fs::create_dir(root.path().join("tree/cache")).unwrap();
    fs::create_dir(root.path().join("tree/empty-dir")).unwrap();
    fs::write(root.path().join("tree/cache/file.tmp"), "cache\n").unwrap();
    fs::write(root.path().join("tree/keep.txt"), "keep\n").unwrap();
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

fn current_fstype(path: &Path) -> OsString {
    let output = Command::new("find")
        .arg(path)
        .args(["-maxdepth", "0", "-printf", "%F"])
        .output()
        .unwrap();
    assert!(output.status.success());

    OsString::from(String::from_utf8(output.stdout).unwrap().trim().to_owned())
}

fn assert_newermt_literal_rejection_matches_gnu(root: &Path, raw: &str) {
    let args = vec![path_arg(root), "-newermt".into(), raw.into()];

    let expected = Command::new("find")
        .env("LC_ALL", "C")
        .env("TZ", PRINTF_TIME_TZ)
        .args(&args)
        .output()
        .unwrap();

    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .env("LC_ALL", "C")
        .env("TZ", PRINTF_TIME_TZ)
        .args(&args)
        .output()
        .unwrap();

    assert!(!expected.status.success(), "{raw}");
    assert_eq!(actual.status.success(), expected.status.success(), "{raw}");
    assert!(actual.stdout.is_empty(), "{raw}");
    assert!(expected.stdout.is_empty(), "{raw}");
}

fn proc_path_without_birth_time() -> Option<&'static Path> {
    let candidate = Path::new("/proc/self/status");
    match read_birth_time(candidate, true) {
        Ok(None) => Some(candidate),
        _ => None,
    }
}

fn snapshot_tree(root: &Path) -> BTreeSet<String> {
    fn visit(base: &Path, path: &Path, out: &mut BTreeSet<String>) {
        let relative = path.strip_prefix(base).unwrap();
        let label = if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.display().to_string()
        };
        out.insert(label);

        if fs::symlink_metadata(path).unwrap().file_type().is_dir() {
            for child in fs::read_dir(path).unwrap() {
                visit(base, &child.unwrap().path(), out);
            }
        }
    }

    if !root.exists() {
        return BTreeSet::from(["<missing>".to_string()]);
    }

    let mut out = BTreeSet::new();
    visit(root, root, &mut out);
    out
}

fn normalize_root(bytes: &[u8], root: &Path) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace(&root.display().to_string(), "<ROOT>")
}

#[test]
fn ordered_exec_semicolon_matches_gnu_find_exactly() {
    let root = build_exec_tree();
    assert_matches_gnu_exact(&[
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-exec".into(),
        "false".into(),
        "{}".into(),
        ";".into(),
        "-o".into(),
        "-print".into(),
    ]);
}

#[test]
fn ordered_exec_plus_matches_gnu_find_exactly() {
    let root = build_exec_tree();
    assert_matches_gnu_exact(&[
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-exec".into(),
        "false".into(),
        "{}".into(),
        "+".into(),
        "-print".into(),
    ]);
}

#[test]
fn ordered_execdir_semicolon_matches_gnu_find_exactly() {
    let root = build_execdir_tree();
    let args_sets = vec![
        vec![
            path_arg(&root.path().join("dir")),
            "-name".into(),
            "*.txt".into(),
            "-execdir".into(),
            "printf".into(),
            "%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        vec![
            path_arg(&root.path().join("dir")),
            "-name".into(),
            "*.txt".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "pwd; printf '%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_execdir_plus_matches_gnu_find_exactly() {
    let root = build_execdir_tree();
    let args = vec![
        path_arg(&root.path().join("dir")),
        "-name".into(),
        "*.txt".into(),
        "-execdir".into(),
        "sh".into(),
        "-c".into(),
        "printf '%s|' \"$PWD\"; printf '%s ' \"$@\"; printf '\\n'".into(),
        "sh".into(),
        "{}".into(),
        "+".into(),
    ];

    assert_matches_gnu_exact(&args);
}

#[test]
fn ordered_execdir_on_symlink_roots_matches_gnu_exactly() {
    let root = build_execdir_tree();
    for flag in ["-P", "-H", "-L"] {
        assert_matches_gnu_exact(&[
            flag.into(),
            path_arg(&root.path().join("start/link")),
            "-maxdepth".into(),
            "0".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "pwd; printf '%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ]);
    }
}

#[test]
fn gnu_ok_eof_still_prints_prompt_and_skips_child() {
    let root = build_exec_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-ok".into(),
        "printf".into(),
        "RUN:%s\\n".into(),
        "{}".into(),
        ";".into(),
    ];

    assert_matches_gnu_exact_with_input(&args, b"", true);
}

#[test]
fn gnu_okdir_prompts_with_matched_path_but_executes_with_dirlocal_basename() {
    let root = build_execdir_tree();
    let args = vec![
        path_arg(root.path()),
        "-name".into(),
        "alpha.txt".into(),
        "-okdir".into(),
        "sh".into(),
        "-c".into(),
        "printf 'PWD:%s\\nARG:%s\\n' \"$PWD\" \"$1\"".into(),
        "sh".into(),
        "{}".into(),
        ";".into(),
    ];

    assert_matches_gnu_exact_with_input(&args, b"yes\n", true);
}

#[test]
fn gnu_ok_plus_is_rejected() {
    let root = build_exec_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-ok".into(),
        "echo".into(),
        "{}".into(),
        "+".into(),
    ];

    let expected = Command::new("find")
        .env("LC_ALL", "C")
        .args(&args)
        .output()
        .unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .env("LC_ALL", "C")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert!(String::from_utf8(actual.stderr).unwrap().contains("`-ok`"));
}

#[test]
fn unsafe_execdir_path_rejection_matches_gnu_semantics() {
    let root = build_execdir_tree();
    for command in [
        vec!["echo", "{}", ";"],
        vec!["/bin/true", "{}", ";"],
        vec!["./sub/cmd", "{}", ";"],
    ] {
        let mut args = vec![
            path_arg(&root.path().join("dir")),
            "-name".into(),
            "*.txt".into(),
            "-execdir".into(),
        ];
        args.extend(command.into_iter().map(Into::into));

        let expected = Command::new("find")
            .env("PATH", ".:/usr/bin:/bin")
            .args(&args)
            .output()
            .unwrap();
        let actual = Command::cargo_bin("findoxide")
            .unwrap()
            .env("FINDOXIDE_WORKERS", "1")
            .env("PATH", ".:/usr/bin:/bin")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(actual.status.success(), expected.status.success());
        assert!(actual.stdout.is_empty());
        assert!(expected.stdout.is_empty());
        assert!(!actual.stderr.is_empty());
        assert!(!expected.stderr.is_empty());
    }
}

#[test]
fn ordered_quit_matches_gnu_find_exactly() {
    let root = build_exec_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-print".into(), "-quit".into()],
        vec![path_arg(root.path()), "-quit".into(), "-print".into()],
        vec![
            path_arg(root.path()),
            "-name".into(),
            "beta.txt".into(),
            "-quit".into(),
            "-o".into(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-name".into(),
            "beta.txt".into(),
            "-print".into(),
            "-quit".into(),
            "-o".into(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "Q:%s\\n".into(),
            "{}".into(),
            "+".into(),
            "-quit".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn fprint_matches_gnu_for_successful_ordered_runs() {
    let root = build_printf_tree();
    assert_file_output_matches_gnu_with_env(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
        ],
        "-fprint",
        1,
        "paths.txt",
        &[],
    );
}

#[test]
fn fprintf_matches_gnu_for_successful_ordered_runs() {
    let root = build_printf_tree();
    assert_file_output_matches_gnu_with_env(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
        ],
        "-fprintf",
        1,
        "printf.txt",
        &["[%P][%y]\\n"],
    );
}

#[test]
fn fprintf_literal_escapes_match_gnu_for_successful_ordered_runs() {
    let root = build_printf_tree();
    assert_file_output_matches_gnu_with_env(
        &[
            path_arg(root.path().join("dir/file.txt").as_path()),
            "-maxdepth".into(),
            "0".into(),
        ],
        "-fprintf",
        1,
        "escapes.bin",
        &["A\\a\\101\\cB"],
    );
}

#[test]
fn fprint0_matches_gnu_for_successful_ordered_runs() {
    let root = build_printf_tree();
    assert_file_output_matches_gnu_with_env(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-maxdepth".into(),
            "1".into(),
        ],
        "-fprint0",
        1,
        "paths.bin",
        &[],
    );
}

#[test]
fn reports_parse_errors_nonzero() {
    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .args(["(", "-name", "*.rs"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("expected `)`")
    );
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
fn ordered_structural_traversal_controls_match_gnu_find_exactly() {
    let root = build_prune_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-depth".into(), "-print".into()],
        vec![
            path_arg(root.path()),
            "-name".into(),
            "vendor".into(),
            "-prune".into(),
            "-o".into(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "d".into(),
            "-name".into(),
            "vendor".into(),
            "-prune".into(),
            "-o".into(),
            "-name".into(),
            "*.rs".into(),
            "-print".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_delete_matches_gnu_output_exit_and_resulting_state() {
    let expected = build_delete_tree();
    let actual = build_delete_tree();

    let expected_root = expected.path().join("tree");
    let actual_root = actual.path().join("tree");
    let args_expected = vec![
        path_arg(&expected_root),
        "-mindepth".into(),
        "1".into(),
        "-delete".into(),
    ];
    let args_actual = vec![
        path_arg(&actual_root),
        "-mindepth".into(),
        "1".into(),
        "-delete".into(),
    ];

    let expected_output = Command::new("find").args(&args_expected).output().unwrap();
    let actual_output = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .args(&args_actual)
        .output()
        .unwrap();

    assert_eq!(actual_output.status.code(), expected_output.status.code());
    assert_eq!(
        normalize_root(&actual_output.stdout, &actual_root),
        normalize_root(&expected_output.stdout, &expected_root),
    );
    assert_eq!(
        normalize_root(&actual_output.stderr, &actual_root),
        normalize_root(&expected_output.stderr, &expected_root),
    );
    assert_eq!(snapshot_tree(&actual_root), snapshot_tree(&expected_root));
}

#[test]
fn ordered_fstype_matches_gnu_find_exactly() {
    let root = build_tree();
    let host_type = current_fstype(root.path());
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-fstype".into(),
            host_type.clone(),
            "-print".into(),
        ],
        vec![
            path_arg(root.path()),
            "-fstype".into(),
            "definitely-not-a-real-fstype".into(),
            "-print".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
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
fn ordered_printf_subset_matches_gnu_find_exactly() {
    let root = build_printf_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-printf".into(),
            "[%P][%f][%h][%d]\\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%f][%y][%s][%m]\\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "l".into(),
            "-printf".into(),
            "[%f][%y][%l]\\n".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn parallel_printf_subset_matches_gnu_find_as_sets() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path()),
        "-printf".into(),
        "[%P][%f][%h][%d]\\n".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_printf_literal_escapes_match_gnu_find_exactly() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path().join("dir/file.txt").as_path()),
        "-maxdepth".into(),
        "0".into(),
        "-printf".into(),
        "A\\aB\\bC\\fD\\nE\\rF\\tG\\vH\\101\\040\\0123\\400".into(),
    ];

    assert_matches_gnu_exact(&args);
}

#[test]
fn parallel_printf_literal_escape_subset_matches_gnu_find_as_sets() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-printf".into(),
        "[%f][\\101][\\t][\\040]\\n".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_printf_target_type_matches_gnu_find_exactly() {
    let root = build_printf_target_type_tree();
    let args = vec![
        path_arg(root.path()),
        "-mindepth".into(),
        "1".into(),
        "-maxdepth".into(),
        "1".into(),
        "-printf".into(),
        "[%f][%y][%Y]\\n".into(),
    ];

    assert_matches_gnu_exact(&args);
}

#[test]
fn parallel_printf_target_type_matches_gnu_find_as_sets() {
    let root = build_printf_target_type_tree();
    let args = vec![
        path_arg(root.path()),
        "-mindepth".into(),
        "1".into(),
        "-maxdepth".into(),
        "1".into(),
        "-printf".into(),
        "[%f][%y][%Y]\\n".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_printf_sparseness_matches_gnu_find_exactly() {
    let root = build_printf_sparseness_tree();
    let args = vec![
        path_arg(root.path()),
        "-mindepth".into(),
        "1".into(),
        "-maxdepth".into(),
        "1".into(),
        "-printf".into(),
        "[%f][%s][%b][%S]\\n".into(),
    ];

    assert_matches_gnu_exact(&args);
}

#[test]
fn parallel_printf_sparseness_matches_gnu_find_as_sets() {
    let root = build_printf_sparseness_tree();
    let args = vec![
        path_arg(root.path()),
        "-mindepth".into(),
        "1".into(),
        "-maxdepth".into(),
        "1".into(),
        "-printf".into(),
        "[%f][%s][%b][%S]\\n".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_printf_unknown_escape_warnings_match_gnu_with_normalized_program_name() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path().join("dir/file.txt").as_path()),
        "-maxdepth".into(),
        "0".into(),
        "-printf".into(),
        "X\\qY\\xZ".into(),
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
    assert_eq!(
        normalize_warning_program(&actual.stderr),
        normalize_warning_program(&expected.stderr)
    );
}

#[test]
fn ordered_printf_unknown_escape_warnings_match_gnu_for_zero_match_runs() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path()),
        "-name".into(),
        "definitely-no-match".into(),
        "-printf".into(),
        "X\\q".into(),
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
    assert_eq!(
        normalize_warning_program(&actual.stderr),
        normalize_warning_program(&expected.stderr)
    );
}

#[test]
fn ordered_printf_expanded_subset_matches_gnu_find_exactly() {
    let root = build_printf_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-printf".into(),
            "[%H][%P][%i][%n][%D][%M]\\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%u][%U][%g][%G][%b][%k][%F]\\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%10i][%-10u][%.2F][%010d][%#10m]\\n".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn parallel_printf_expanded_subset_matches_gnu_find_as_sets() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path()),
        "-printf".into(),
        "[%H][%P][%i][%n][%D][%b][%k][%M][%u][%U][%g][%G][%F]\\n".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_printf_time_directives_match_gnu_find_exactly() {
    let root = build_printf_tree();
    let status = Command::new("touch")
        .env("TZ", PRINTF_TIME_TZ)
        .args(["-a", "-m", "-d", "2024-03-04 13:06:07.123456789"])
        .arg(root.path().join("dir/file.txt"))
        .status()
        .unwrap();
    assert!(status.success());

    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%a][%c][%t]\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%TY][%Tm][%Td][%TH][%TM][%TS][%TZ][%Tz]\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%Ta][%TA][%TB][%Tp][%T@][%T+]\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%.3Ta][%10Ta][%.5T@][%010Ta][%+10T@][%#10T+]\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%AY][%Cm][%Td][%CH:%CM:%CS]\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-printf".into(),
            "[%Tb][%Th][%Tc][%TD][%TF][%Tg][%TG][%TI][%Tj][%Tr][%TR][%Tu][%TU][%Tw][%TW][%Tx][%TX][%Ty][%TV][%Tt]\n".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact_with_env(&args);
    }
}

#[test]
fn parallel_printf_time_directives_match_gnu_find_as_sets() {
    let root = build_printf_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-printf".into(),
        "[%f][%TY-%Tm-%Td][%TH:%TM:%TS][%T@][%T+][%Tb][%Tc][%TR][%TX][%TV]\n".into(),
    ];

    assert_matches_gnu_as_sets_with_env(&args);
}

#[test]
fn ordered_printf_birth_time_renders_empty_fields_when_unavailable() {
    let Some(proc_path) = proc_path_without_birth_time() else {
        return;
    };

    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .env("LC_ALL", "C")
        .env("TZ", PRINTF_TIME_TZ)
        .args([
            path_arg(proc_path),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%B][%BY][%B@][%B+]\n".into(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "[][][][]\n");
}

#[test]
fn ordered_printf_birth_time_renders_linux_birth_data_when_available() {
    let root = build_printf_tree();
    let birth_path = root.path().join("dir/file.txt");
    if read_birth_time(&birth_path, true).unwrap().is_none() {
        return;
    }

    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .env("LC_ALL", "C")
        .env("TZ", PRINTF_TIME_TZ)
        .args([
            path_arg(&birth_path),
            "-maxdepth".into(),
            "0".into(),
            "-printf".into(),
            "[%B][%BY][%Bm][%Bd][%B@][%B+]\n".into(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('['));
    assert!(stdout.contains("][20"));
    assert!(stdout.contains("][0"));
    assert!(stdout.contains('.'));
    assert!(stdout.contains('+'));
    assert!(stdout.ends_with("]\n"));
}

#[test]
fn parallel_prune_matches_gnu_as_a_set() {
    let root = build_prune_tree();
    let args = vec![
        path_arg(root.path()),
        "-name".into(),
        "vendor".into(),
        "-prune".into(),
        "-o".into(),
        "-print".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn parallel_fstype_matches_gnu_as_a_set() {
    let root = build_tree();
    let host_type = current_fstype(root.path());
    let args = vec![
        path_arg(root.path()),
        "-fstype".into(),
        host_type,
        "-print".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_access_predicates_match_gnu_find_exactly() {
    let root = build_access_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-readable".into(), "-print".into()],
        vec![path_arg(root.path()), "-writable".into(), "-print".into()],
        vec![path_arg(root.path()), "-executable".into(), "-print".into()],
        vec![
            path_arg(&root.path().join("searchable-dir")),
            "-maxdepth".into(),
            "0".into(),
            "-executable".into(),
            "-print".into(),
        ],
        vec![
            "-P".into(),
            path_arg(root.path()),
            "-type".into(),
            "l".into(),
            "-readable".into(),
            "-print".into(),
        ],
        vec![
            "-H".into(),
            path_arg(root.path().join("root-link").as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-readable".into(),
            "-print".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn parallel_access_predicates_match_gnu_find_as_sets() {
    let root = build_access_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "(".into(),
            "-readable".into(),
            "-o".into(),
            "-executable".into(),
            ")".into(),
            "-print".into(),
        ],
        vec![
            "-L".into(),
            path_arg(root.path()),
            "-name".into(),
            "*link".into(),
            "-readable".into(),
            "-print".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_as_sets(&args);
    }
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
fn ls_matches_gnu_for_weird_names_and_follow_modes() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("plain"), "x").unwrap();
    fs::write(root.path().join("space name"), "x").unwrap();
    fs::write(root.path().join("tab\tname"), "x").unwrap();
    fs::write(root.path().join("line\nbreak"), "x").unwrap();
    fs::write(root.path().join("old-file"), "x").unwrap();
    let touch_status = Command::new("touch")
        .args(["-t", "202001020304.05"])
        .arg(root.path().join("old-file"))
        .status()
        .unwrap();
    assert!(touch_status.success());
    unix_fs::symlink("plain", root.path().join("root-link")).unwrap();
    unix_fs::symlink("space name", root.path().join("space-link")).unwrap();

    assert_matches_gnu_exact_with_env(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-ls".into(),
    ]);
    assert_matches_gnu_exact_with_env(&[
        "-L".into(),
        path_arg(root.path().join("root-link").as_path()),
        "-maxdepth".into(),
        "0".into(),
        "-ls".into(),
    ]);
    assert_matches_gnu_exact_with_env(&[
        "-H".into(),
        path_arg(root.path().join("root-link").as_path()),
        "-maxdepth".into(),
        "0".into(),
        "-ls".into(),
    ]);
    assert_matches_gnu_as_sets_with_env(&[
        path_arg(root.path()),
        "-maxdepth".into(),
        "1".into(),
        "-ls".into(),
    ]);
    assert_file_output_matches_gnu_with_env(
        &[path_arg(root.path()), "-maxdepth".into(), "1".into()],
        "-fls",
        1,
        "listing.ls",
        &[],
    );
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
        vec![path_arg(root.path()), "-mmin".into(), "16".into()],
        vec![path_arg(root.path()), "-mmin".into(), "+15".into()],
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
fn ordered_fractional_time_predicates_match_gnu_find_exactly() {
    let root = build_size_time_tree();
    let args_sets = vec![
        vec![path_arg(root.path()), "-mmin".into(), "0.5".into()],
        vec![path_arg(root.path()), "-mmin".into(), "+0.1".into()],
        vec![path_arg(root.path()), "-mtime".into(), "1.5".into()],
        vec![
            path_arg(root.path()),
            "-daystart".into(),
            "-mtime".into(),
            "0.5".into(),
        ],
        vec![
            path_arg(root.path()),
            "-daystart".into(),
            "-mtime".into(),
            "-1.5".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_newermt_literal_time_acceptance_matches_gnu_find_exactly() {
    let root = build_size_time_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "@1700000000.25".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15 1234".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15 12:34".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34:56".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15 12:34:56.123456789".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34:56Z".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34:56+08".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34:56+0800".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "2026-04-15T12:34:56+08:00".into(),
        ],
        vec![path_arg(root.path()), "-newermt".into(), "20260415".into()],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415 1234".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415 12:34".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415 12:34:56".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415T1234".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415T12:34".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415T12:34:56".into(),
        ],
        vec![
            path_arg(root.path()),
            "-newermt".into(),
            "20260415T12:34:56.25".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact_with_env(&args);
    }
}

#[test]
fn ordered_newermt_literal_time_rejection_matches_gnu_find() {
    let root = build_size_time_tree();

    for raw in [
        "202604151234",
        "20260415123456",
        "202604151234.56",
        "2026-04-15T12:34.5",
        "20260415 123456",
        "20260415 123456.25",
        "20260415T12:34Z",
        "20260415T12:34:56Z",
        "20260415T12:34+08:00",
        "20260415T12:34:56+08:00",
        "20260415 1234+08:00",
        "2026-04-15T1234",
        "2026-04-15T123456",
        "2026-04-15 123456",
    ] {
        assert_newermt_literal_rejection_matches_gnu(root.path(), raw);
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
    let args_sets = vec![
        vec![path_arg(root.path()), "-empty".into()],
        vec!["-L".into(), path_arg(root.path()), "-empty".into()],
        // GNU differential `-used` checks stay on regular files only. A `find`
        // traversal mutates directory atime, so shared-tree directory comparisons
        // are order-sensitive. Directory `-empty`/`-used` interaction is covered
        // by deterministic evaluator tests instead.
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-used".into(),
            "0".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-used".into(),
            "1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-used".into(),
            "-1".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }

    if gnu_supports_birth_time_predicates(root.path()) {
        assert_matches_gnu_exact(&[
            path_arg(root.path()),
            "-newerBt".into(),
            "@1700000000.25".into(),
        ]);

        if read_birth_time(&root.path().join("reference-file"), true)
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
}

#[test]
fn parallel_stage9_read_only_tail_matches_gnu_find_as_sets() {
    let root = build_read_only_tail_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "(".into(),
        "-empty".into(),
        "-a".into(),
        "-type".into(),
        "f".into(),
        ")".into(),
        "-o".into(),
        "(".into(),
        "-used".into(),
        "1".into(),
        "-a".into(),
        "-type".into(),
        "f".into(),
        ")".into(),
        "-o".into(),
        "(".into(),
        "-used".into(),
        "-1".into(),
        "-a".into(),
        "-type".into(),
        "f".into(),
        ")".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn ordered_fractional_used_predicates_match_gnu_find_exactly() {
    let root = build_read_only_tail_tree();
    let args = vec![
        path_arg(root.path()),
        "-type".into(),
        "f".into(),
        "-used".into(),
        "0.5".into(),
    ];

    assert_matches_gnu_exact(&args);
}

#[test]
fn ordered_regex_predicates_match_gnu_find_exactly() {
    let root = build_regex_tree();
    let src_alias = OsString::from(format!("{}/src/*", root.path().display()));
    let readme_alias = OsString::from(format!("{}/readme*", root.path().display()));
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-regex".into(),
            r".*/\(src\|docs\)/.*".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            ".*/(src|docs)/.*".into(),
        ],
        vec![
            path_arg(root.path()),
            "-iregex".into(),
            ".*/readme\\.md".into(),
        ],
        vec![path_arg(root.path()), "-wholename".into(), src_alias],
        vec![path_arg(root.path()), "-iwholename".into(), readme_alias],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_expanded_regex_subset_matches_gnu_find_exactly() {
    let root = build_regex_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/src/\(lib\|main\)\.rs".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/src/[[:alpha:]]\{3\}\.rs".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/[[:upper:]][[:alpha:]]*\.MD".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/[[:upper:]][[:alpha:]]*\.MD".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_emacs_regex_edge_semantics_match_gnu_find_exactly() {
    let root = build_emacs_regex_edge_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            ".*/a+".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/a\+".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            ".*/a^b".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/\a".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*[\\]]".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn ordered_bre_intervals_with_omitted_lower_bounds_match_gnu_find_exactly() {
    let root = build_bre_interval_tree();
    let literal_root = regex::escape(&root.path().to_string_lossy());
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            format!(r"{literal_root}/a\{{,2\}}b").into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            format!(r"{literal_root}/a\{{,\}}b").into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            format!(r"{literal_root}/a\{{,2\}}b").into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            format!(r"{literal_root}/a\{{,\}}b").into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn parallel_regex_predicates_match_gnu_find_as_sets() {
    let root = build_regex_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regex".into(),
        r".*/\(src\|docs\)/.*".into(),
        "-o".into(),
        "-iregex".into(),
        ".*/readme\\.md".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn parallel_expanded_regex_subset_matches_gnu_find_as_sets() {
    let root = build_regex_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "posix-basic".into(),
        "-regex".into(),
        r".*/src/\(lib\|main\)\.rs".into(),
        "-o".into(),
        "-regextype".into(),
        "posix-extended".into(),
        "-regex".into(),
        r".*/[[:upper:]][[:alpha:]]*\.MD".into(),
        "-o".into(),
        "-regextype".into(),
        "emacs".into(),
        "-regex".into(),
        r".*/[[:upper:]][[:alpha:]]*\.MD".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn regex_foundation_matrix_ordered_gnu_extensions_match_gnu_find_exactly() {
    let root = build_regex_extension_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            ".*/paren)".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/\(\+foo\)".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/\(.\)\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/\<foo\>".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn regex_foundation_matrix_parallel_gnu_extensions_match_gnu_find_as_sets() {
    let root = build_regex_extension_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "posix-basic".into(),
        "-regex".into(),
        r".*/\(.\)\1".into(),
        "-o".into(),
        "-regextype".into(),
        "posix-extended".into(),
        "-regex".into(),
        r".*/\<foo\>".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

fn build_emacs_regex_followup_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in ["a", "aa", "ab", "a^b", "abab", "abcd", "cdcd"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

#[test]
fn regex_emacs_followup_matrix_ordered_matches_gnu_find_exactly() {
    let root = build_emacs_regex_followup_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/\(.\)\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/\(ab\|cd\)\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/\(a+\|a\^b\)".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn regex_emacs_followup_matrix_parallel_matches_gnu_find_as_sets() {
    let root = build_emacs_regex_followup_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "emacs".into(),
        "-regex".into(),
        r".*/\(.\)\1".into(),
        "-o".into(),
        "-regextype".into(),
        "emacs".into(),
        "-regex".into(),
        r".*/\(a+\|a\^b\)".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

fn build_regex_bracket_review_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in ["a", "b", "c", "d", "z", "-", "\\"] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

fn build_gnu_regex_hardening_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    for name in [
        "a", "aa", "ab", "abab", "abcd", "cdcd", "foo", "foobar", "paren)", "+foo", "?foo", "-",
        "\\",
    ] {
        fs::write(root.path().join(name), "x\n").unwrap();
    }
    root
}

#[test]
fn gnu_review_followup_ordered_bracket_semantics_match_gnu_find_exactly() {
    let root = build_regex_bracket_review_tree();
    let args_sets = vec![
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/[a\b]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/[a\b]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/[a-c]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/[a-c]".into(),
        ],
    ];

    for args in args_sets {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn gnu_review_followup_parallel_bracket_semantics_match_gnu_find_as_sets() {
    let root = build_regex_bracket_review_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "posix-basic".into(),
        "-regex".into(),
        r".*/[a\b]".into(),
        "-o".into(),
        "-regextype".into(),
        "posix-extended".into(),
        "-regex".into(),
        r".*/[a-c]".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}

#[test]
fn gnu_review_followup_backward_ranges_are_rejected_like_gnu_find() {
    let root = build_regex_bracket_review_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/[z-a]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/[z-a]".into(),
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
        assert!(actual.status.code() != Some(0));
        assert!(actual.stdout.is_empty());
        assert!(expected.stdout.is_empty());
        assert!(!actual.stderr.is_empty());
        assert!(!expected.stderr.is_empty());
    }
}

#[test]
fn gnu_hardening_invalid_regex_outcomes_match_gnu_find() {
    let root = build_gnu_regex_hardening_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/a\{2,1\}".into(),
        ],
        vec![
            path_arg(root.path()),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/a{2,1}".into(),
        ],
    ] {
        assert_matches_gnu_regex_outcome(&args);
    }
}

#[test]
fn gnu_hardening_invalid_regex_outcomes_match_gnu_find_in_parallel_mode() {
    let root = build_gnu_regex_hardening_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "posix-basic".into(),
        "-regex".into(),
        r".*/\1".into(),
        "-o".into(),
        "-regextype".into(),
        "posix-extended".into(),
        "-regex".into(),
        r".*/a{2,1}".into(),
        ")".into(),
    ];

    assert_matches_gnu_regex_outcome_as_sets(&args);
}

#[test]
fn gnu_hardening_success_ordered_matrix_matches_gnu_find() {
    let root = build_gnu_regex_hardening_tree();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            ".*/paren)".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/\(\+foo\)".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/\(\?foo\)".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/[a\b]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/[a\b]".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/(ab|cd)\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "emacs".into(),
            "-regex".into(),
            r".*/\(ab\|cd\)\1".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            r".*/a{2,}".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-mindepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-basic".into(),
            "-regex".into(),
            r".*/a\{2,\}".into(),
        ],
    ] {
        assert_matches_gnu_exact(&args);
    }
}

#[test]
fn gnu_hardening_success_parallel_matrix_matches_gnu_find_as_sets() {
    let root = build_gnu_regex_hardening_tree();
    let args = vec![
        path_arg(root.path()),
        "(".into(),
        "-regextype".into(),
        "posix-basic".into(),
        "-regex".into(),
        r".*/[a\b]".into(),
        "-o".into(),
        "-regextype".into(),
        "posix-extended".into(),
        "-regex".into(),
        r".*/(ab|cd)\1".into(),
        "-o".into(),
        "-regextype".into(),
        "emacs".into(),
        "-regex".into(),
        r".*/\(ab\|cd\)\1".into(),
        ")".into(),
    ];

    assert_matches_gnu_as_sets(&args);
}
