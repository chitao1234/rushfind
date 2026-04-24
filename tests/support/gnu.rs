use super::{lines, rushfind_command_with_workers};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::{Once, OnceLock};
use tempfile::tempdir;

pub const PRINTF_TIME_TZ: &str = "Asia/Shanghai";
static GNU_FIND_PROGRAM: OnceLock<Option<OsString>> = OnceLock::new();
static GNU_FIND_MISSING_REPORTED: Once = Once::new();
const APPROXIMATE_INTERACTIVE_LOCALE_WARNING: &str =
    "rfd: warning: interactive locale behavior is approximate on this platform";

fn apply_common_env(command: &mut Command) {
    command.env("LC_ALL", "C").env("TZ", PRINTF_TIME_TZ);
}

fn resolve_program_on_path(program: &str) -> Option<PathBuf> {
    let current_dir = env::current_dir().ok()?;
    let path = env::var_os("PATH")?;

    env::split_paths(&path).find_map(|dir| {
        let candidate_dir = if dir.is_absolute() {
            dir
        } else {
            current_dir.join(dir)
        };
        let candidate = candidate_dir.join(program);
        if !candidate.is_file() {
            return None;
        }

        Some(fs::canonicalize(&candidate).unwrap_or(candidate))
    })
}

fn detect_gnu_find_program() -> Option<OsString> {
    for program in ["gfind", "find"] {
        let Some(candidate) = resolve_program_on_path(program) else {
            continue;
        };
        let Ok(output) = Command::new(&candidate).arg("--version").output() else {
            continue;
        };
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("GNU findutils")
        {
            return Some(candidate.into_os_string());
        }
    }

    None
}

fn report_missing_gnu_find() {
    GNU_FIND_MISSING_REPORTED.call_once(|| {
        eprintln!(
            "skipping GNU comparison test: GNU `find` not available (searched for `gfind` and GNU `find`)"
        );
    });
}

pub fn ensure_gnu_find() -> bool {
    if GNU_FIND_PROGRAM
        .get_or_init(detect_gnu_find_program)
        .is_some()
    {
        true
    } else {
        report_missing_gnu_find();
        false
    }
}

pub fn gnu_find_program() -> Option<&'static OsStr> {
    GNU_FIND_PROGRAM
        .get_or_init(detect_gnu_find_program)
        .as_deref()
}

pub fn gnu_find_command() -> Option<Command> {
    let program = gnu_find_program()?;
    Some(Command::new(program))
}

pub fn gnu_find_output(args: &[OsString], with_env: bool) -> Option<Output> {
    let mut command = gnu_find_command()?;
    if with_env {
        apply_common_env(&mut command);
    }
    Some(command.args(args).output().unwrap())
}

fn rushfind_output(args: &[OsString], workers: usize, with_env: bool) -> Output {
    let mut command = rushfind_command_with_workers(workers);
    if with_env {
        apply_common_env(&mut command);
    }
    command.args(args).output().unwrap()
}

pub fn assert_matches_gnu_exact(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, false) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 1, false);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

pub fn assert_matches_gnu_as_sets(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, false) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 4, false);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
}

pub fn assert_matches_gnu_exact_with_env(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, true) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 1, true);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

pub fn assert_matches_gnu_exact_with_input(args: &[OsString], input: &[u8], with_env: bool) {
    let Some(mut expected) = gnu_find_command() else {
        report_missing_gnu_find();
        return;
    };
    if with_env {
        apply_common_env(&mut expected);
    }
    expected
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut expected = expected.spawn().unwrap();
    if !input.is_empty() {
        expected.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(expected.stdin.take());
    let expected = expected.wait_with_output().unwrap();

    let mut actual = rushfind_command_with_workers(1);
    actual
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if with_env {
        apply_common_env(&mut actual);
    }
    let mut actual = actual.spawn().unwrap();
    if !input.is_empty() {
        actual.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(actual.stdin.take());
    let actual = actual.wait_with_output().unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(
        strip_rushfind_only_differential_warnings(&actual.stderr),
        expected.stderr
    );
}

pub fn assert_matches_gnu_as_sets_with_env(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, true) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 4, true);

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
    assert_eq!(lines(&actual.stderr), lines(&expected.stderr));
}

pub fn assert_matches_gnu_regex_outcome(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, false) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 1, false);

    assert_eq!(
        actual.status.success(),
        expected.status.success(),
        "args: {:?}",
        args
    );
    assert_eq!(
        actual.status.code(),
        expected.status.code(),
        "args: {:?}",
        args
    );
    assert_eq!(actual.stdout, expected.stdout, "args: {:?}", args);

    if expected.status.success() {
        assert_eq!(actual.stderr, expected.stderr, "args: {:?}", args);
    } else {
        assert!(!expected.stderr.is_empty(), "args: {:?}", args);
        assert!(!actual.stderr.is_empty(), "args: {:?}", args);
    }
}

pub fn assert_matches_gnu_regex_outcome_as_sets(args: &[OsString]) {
    let Some(expected) = gnu_find_output(args, false) else {
        report_missing_gnu_find();
        return;
    };
    let actual = rushfind_output(args, 4, false);

    assert_eq!(
        actual.status.success(),
        expected.status.success(),
        "args: {:?}",
        args
    );
    assert_eq!(
        actual.status.code(),
        expected.status.code(),
        "args: {:?}",
        args
    );
    assert_eq!(
        lines(&actual.stdout),
        lines(&expected.stdout),
        "args: {:?}",
        args
    );

    if expected.status.success() {
        assert_eq!(
            lines(&actual.stderr),
            lines(&expected.stderr),
            "args: {:?}",
            args
        );
    } else {
        assert!(!expected.stderr.is_empty(), "args: {:?}", args);
        assert!(!actual.stderr.is_empty(), "args: {:?}", args);
    }
}

pub fn assert_file_output_matches_gnu_with_env(
    args: &[OsString],
    action: &str,
    workers: usize,
    output_name: &str,
    trailing_args: &[&str],
) {
    let out_dir = tempdir().unwrap();
    let gnu_out = out_dir.path().join(format!("gnu-{output_name}"));
    let oxide_out = out_dir.path().join(format!("oxide-{output_name}"));

    let Some(mut expected_command) = gnu_find_command() else {
        report_missing_gnu_find();
        return;
    };
    apply_common_env(&mut expected_command);
    let expected = expected_command
        .args(args)
        .arg(action)
        .arg(&gnu_out)
        .args(trailing_args)
        .output()
        .unwrap();

    let mut actual_command = rushfind_command_with_workers(workers);
    apply_common_env(&mut actual_command);
    let actual = actual_command
        .args(args)
        .arg(action)
        .arg(&oxide_out)
        .args(trailing_args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(fs::read(&oxide_out).unwrap(), fs::read(&gnu_out).unwrap());
}

pub fn normalize_warning_program(bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| {
            if let Some(index) = line.find("warning: ") {
                line[index..].to_string()
            } else {
                line.strip_prefix("rfd: ")
                    .or_else(|| line.strip_prefix("gfind: "))
                    .or_else(|| line.strip_prefix("find: "))
                    .unwrap_or(line)
                    .to_string()
            }
        })
        .collect()
}

fn strip_rushfind_only_differential_warnings(bytes: &[u8]) -> Vec<u8> {
    let mut filtered = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let without_newline = line.strip_suffix(b"\n").unwrap_or(line);
        if without_newline == APPROXIMATE_INTERACTIVE_LOCALE_WARNING.as_bytes() {
            continue;
        }
        filtered.extend_from_slice(line);
    }
    filtered
}
