mod support;

use assert_cmd::cargo::CommandCargoExt;
use std::fs;
use std::process::Command;
use std::time::Duration;
use support::{
    cargo_bin_output_with_env_and_input_timeout, cargo_bin_output_with_input_timeout,
    cargo_bin_output_with_timeout, first_available_locale, path_arg,
};
use tempfile::tempdir;

#[test]
fn ordered_exec_semicolon_false_short_circuits_later_print_but_exits_zero() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            ";".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn ordered_exec_semicolon_missing_command_is_false_and_allows_or_branch() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();
    let missing = root.path().join("definitely-missing-cmd");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            missing.as_os_str().to_os_string(),
            "{}".into(),
            ";".into(),
            "-o".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("No such file or directory")
    );
}

#[test]
fn ordered_exec_semicolon_rewrites_embedded_placeholders() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "printf".into(),
            "pre{}mid{}post\\n".into(),
            ";".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("pre"));
    assert!(stdout.contains("mid"));
    assert!(stdout.contains("post"));
    assert!(stdout.contains("a.txt"));
}

#[test]
fn ordered_exec_plus_false_keeps_following_print_but_exits_one() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            "+".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
}

#[test]
fn ordered_exec_plus_false_short_circuits_or_branch_like_gnu() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-mindepth".into(),
            "1".into(),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            "+".into(),
            "-o".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn ordered_exec_missing_command_plus_exits_one_after_printing_matches_stage_contract() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();
    let missing = root.path().join("definitely-missing-cmd");

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            missing.as_os_str().to_os_string(),
            "{}".into(),
            "+".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("No such file or directory")
    );
}

#[test]
fn ordered_exec_false_still_blocks_later_print_under_the_pipeline() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "data\n").unwrap();

    let args = vec![
        path_arg(root.path()),
        "-exec".into(),
        "false".into(),
        ";".into(),
        "-print".into(),
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
fn ordered_execdir_semicolon_uses_parent_cwd_and_dot_slash_basename() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "x\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "file.txt".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "pwd; printf '%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.lines().any(|line| line.ends_with("/dir")));
    assert!(stdout.lines().any(|line| line == "./file.txt"));
}

#[test]
fn ordered_execdir_on_symlink_root_uses_typed_parent_and_link_name() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("start")).unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file.txt"), "x\n").unwrap();
    std::os::unix::fs::symlink("../real/file.txt", root.path().join("start/link")).unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
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
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.lines().any(|line| line.ends_with("/start")));
    assert!(stdout.lines().any(|line| line == "./link"));
}

#[test]
fn ordered_execdir_plus_batches_only_within_each_directory() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    fs::create_dir(root.path().join("b")).unwrap();
    fs::write(root.path().join("a/one"), "1\n").unwrap();
    fs::write(root.path().join("a/two"), "2\n").unwrap();
    fs::write(root.path().join("b/three"), "3\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "printf '%s|' \"$PWD\"; printf '%s ' \"$@\"; printf '\\n'".into(),
            "sh".into(),
            "{}".into(),
            "+".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout
            .lines()
            .any(|line| line.contains("/a|") && line.contains("./one") && line.contains("./two"))
    );
    assert!(stdout.lines().any(|line| line.contains("/b|./three ")));
    assert!(
        !stdout
            .lines()
            .any(|line| line.contains("/a|") && line.contains("./three"))
    );
    assert!(
        !stdout
            .lines()
            .any(|line| line.contains("/b|") && (line.contains("./one") || line.contains("./two")))
    );
}

#[test]
fn ordered_execdir_plus_flushes_before_quit() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/a"), "a\n").unwrap();
    fs::write(root.path().join("dir/b"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-execdir".into(),
            "printf".into(),
            "%s\\n".into(),
            "{}".into(),
            "+".into(),
            "-quit".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("./a") || stdout.contains("./b"));
}

#[test]
fn execdir_rejects_unsafe_path_before_traversal_side_effects_even_for_absolute_command() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x\n").unwrap();

    let output = support::cargo_bin_output_with_env_timeout(
        &[
            path_arg(root.path()),
            "-execdir".into(),
            "/bin/true".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        &[("PATH", ".:/usr/bin:/bin")],
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unsafe PATH for `-execdir`")
    );
}

#[test]
fn ordered_ok_yes_runs_child_and_exits_zero() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "printf".into(),
            "RUN:%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        b"yes\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("RUN:"));
    assert!(String::from_utf8(output.stderr).unwrap().contains("? "));
}

#[test]
fn ordered_ok_eof_still_prints_prompt_and_skips_child() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "printf".into(),
            "RUN:%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        b"",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains("? "));
}

#[test]
fn invalid_lc_messages_is_ignored_when_no_ok_action_is_present() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = support::cargo_bin_output_with_env_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-print".into(),
        ],
        1,
        &[
            ("LANG", "C"),
            ("LC_MESSAGES", "definitely-not-a-real-locale.UTF-8"),
        ],
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.txt"));
}

#[test]
fn invalid_lc_messages_falls_back_to_c_for_ok_prompts() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_env_and_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "printf".into(),
            "RUN:%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        &[
            ("LANG", "C"),
            ("LC_MESSAGES", "definitely-not-a-real-locale.UTF-8"),
        ],
        b"yes\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("RUN:"));
    assert!(String::from_utf8(output.stderr).unwrap().contains("? "));
}

#[test]
fn ok_accepts_oui_when_french_locale_is_available() {
    let Some(locale) =
        first_available_locale(&["fr_FR.UTF-8", "fr_FR.utf8", "fr_FR", "fr.UTF-8", "fr"])
    else {
        return;
    };

    let root = tempdir().unwrap();
    fs::write(root.path().join("bonjour.txt"), "salut\n").unwrap();

    let output = cargo_bin_output_with_env_and_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "printf".into(),
            "RUN:%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        &[("LANG", "C"), ("LC_MESSAGES", locale.as_str())],
        b"oui\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("RUN:"));
}

#[test]
fn ordered_okdir_yes_uses_parent_cwd_and_dot_slash_basename() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/file.txt"), "x\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-name".into(),
            "file.txt".into(),
            "-okdir".into(),
            "sh".into(),
            "-c".into(),
            "pwd; printf '%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        b"y\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.lines().any(|line| line.ends_with("/dir")));
    assert!(stdout.lines().any(|line| line == "./file.txt"));
}

#[test]
fn okdir_rejects_unsafe_path_before_prompting() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "x\n").unwrap();

    let output = support::cargo_bin_output_with_env_timeout(
        &[
            path_arg(root.path()),
            "-okdir".into(),
            "/bin/true".into(),
            "{}".into(),
            ";".into(),
        ],
        1,
        &[("PATH", ".:/usr/bin:/bin")],
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unsafe PATH")
    );
}

#[test]
fn parallel_ok_yes_runs_children_under_serialized_sessions() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "sh".into(),
            "-c".into(),
            "printf 'BEGIN:%s\\n' \"$1\"; sleep 0.05; printf 'END:%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        b"yes\nyes\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
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
fn parallel_ok_no_skips_children() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-ok".into(),
            "printf".into(),
            "RUN:%s\\n".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        b"n\nn\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn parallel_okdir_yes_uses_directory_local_execution() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("left")).unwrap();
    fs::create_dir(root.path().join("right")).unwrap();
    fs::write(root.path().join("left/a.txt"), "a\n").unwrap();
    fs::write(root.path().join("right/b.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_input_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-okdir".into(),
            "sh".into(),
            "-c".into(),
            "printf '%s|%s\\n' \"$PWD\" \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        b"yes\nyes\n",
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.lines().any(|line| line.ends_with("/left|./a.txt")));
    assert!(stdout.lines().any(|line| line.ends_with("/right|./b.txt")));
}

#[test]
fn parallel_exec_child_output_is_replayed_in_atomic_chunks() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "sh".into(),
            "-c".into(),
            "printf 'BEGIN:%s\\n' \"$1\"; sleep 0.05; printf 'END:%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
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
fn parallel_execdir_semicolon_replays_each_child_output_atomically() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("left")).unwrap();
    fs::create_dir(root.path().join("right")).unwrap();
    fs::write(root.path().join("left/a.txt"), "a\n").unwrap();
    fs::write(root.path().join("right/b.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "printf 'BEGIN:%s:%s\\n' \"$PWD\" \"$1\"; sleep 0.05; printf 'END:%s:%s\\n' \"$PWD\" \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
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
fn parallel_execdir_plus_never_mixes_directories_within_one_invocation() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    fs::create_dir(root.path().join("b")).unwrap();
    for name in ["a1", "a2", "a3"] {
        fs::write(root.path().join("a").join(name), "a\n").unwrap();
    }
    for name in ["b1", "b2", "b3"] {
        fs::write(root.path().join("b").join(name), "b\n").unwrap();
    }

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-execdir".into(),
            "sh".into(),
            "-c".into(),
            "printf '%s|' \"$PWD\"; printf '%s ' \"$@\"; printf '\\n'".into(),
            "sh".into(),
            "{}".into(),
            "+".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(!lines.is_empty());
    for line in lines {
        let (cwd, args) = line.split_once('|').unwrap();
        let words = args.split_whitespace().collect::<Vec<_>>();
        assert!(!words.is_empty());
        if cwd.ends_with("/a") {
            assert!(
                words
                    .iter()
                    .all(|word| matches!(*word, "./a1" | "./a2" | "./a3"))
            );
        } else if cwd.ends_with("/b") {
            assert!(
                words
                    .iter()
                    .all(|word| matches!(*word, "./b1" | "./b2" | "./b3"))
            );
        } else {
            panic!("unexpected cwd in output: {cwd}");
        }
    }
}

#[test]
fn parallel_execdir_plus_flushes_before_quit() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("dir")).unwrap();
    fs::write(root.path().join("dir/a"), "a\n").unwrap();
    fs::write(root.path().join("dir/b"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-execdir".into(),
            "printf".into(),
            "%s\\n".into(),
            "{}".into(),
            "+".into(),
            "-quit".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("./a") || stdout.contains("./b"));
}

#[test]
fn parallel_exec_plus_false_still_sets_a_nonzero_final_exit() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "false".into(),
            "{}".into(),
            "+".into(),
            "-print".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("alpha.txt")
    );
}

#[test]
fn parallel_exec_child_stderr_is_replayed_in_atomic_chunks() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "sh".into(),
            "-c".into(),
            "printf 'ERR-BEGIN:%s\\n' \"$1\" >&2; sleep 0.05; printf 'ERR-END:%s\\n' \"$1\" >&2"
                .into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let lines = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(lines.chunks_exact(2).all(|chunk| {
        let begin = chunk[0].strip_prefix("ERR-BEGIN:").unwrap();
        let end = chunk[1].strip_prefix("ERR-END:").unwrap();
        begin == end
    }));
}

#[test]
fn parallel_exec_and_print_share_the_broker_without_broken_lines() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "sh".into(),
            "-c".into(),
            "printf 'CMD-BEGIN:%s\\n' \"$1\"; sleep 0.05; printf 'CMD-END:%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
            "-print".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    for line in stdout.lines() {
        assert!(
            line.starts_with("CMD-BEGIN:")
                || line.starts_with("CMD-END:")
                || line.ends_with("alpha.txt")
        );
    }
}

#[test]
fn parallel_v2_exec_semicolon_keeps_child_output_chunks_intact() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha.txt"), "a\n").unwrap();
    fs::write(root.path().join("beta.txt"), "b\n").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(root.path()),
            "-type".into(),
            "f".into(),
            "-exec".into(),
            "sh".into(),
            "-c".into(),
            "printf 'BEGIN:%s\\n' \"$1\"; sleep 0.05; printf 'END:%s\\n' \"$1\"".into(),
            "sh".into(),
            "{}".into(),
            ";".into(),
        ],
        4,
        Duration::from_secs(5),
    );

    let lines = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(lines.len(), 4);
    assert!(lines.chunks_exact(2).all(|chunk| {
        let begin = chunk[0].strip_prefix("BEGIN:").unwrap();
        let end = chunk[1].strip_prefix("END:").unwrap();
        begin == end
    }));
}
