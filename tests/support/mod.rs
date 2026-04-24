#![allow(dead_code, unused_imports)]

#[cfg(unix)]
pub mod gnu;
pub mod planner;
#[cfg(windows)]
pub mod windows;

use assert_cmd::cargo::cargo_bin;
use rushfind::birth::read_birth_time;
use std::collections::BTreeSet;
use std::ffi::CString;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
#[cfg(not(any(target_os = "solaris", target_os = "illumos")))]
use std::mem::MaybeUninit;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Once, OnceLock};
use std::time::Duration;
use wait_timeout::ChildExt;

#[cfg(any(target_os = "solaris", target_os = "illumos"))]
unsafe extern "C" {
    fn rushfind_regexec_match(
        pattern: *const libc::c_char,
        reply: *const libc::c_char,
    ) -> libc::c_int;
}

#[cfg(unix)]
pub use gnu::{
    PRINTF_TIME_TZ, assert_file_output_matches_gnu_with_env, assert_matches_gnu_as_sets,
    assert_matches_gnu_as_sets_with_env, assert_matches_gnu_exact,
    assert_matches_gnu_exact_with_env, assert_matches_gnu_exact_with_input,
    assert_matches_gnu_regex_outcome, assert_matches_gnu_regex_outcome_as_sets, ensure_gnu_find,
    gnu_find_command, gnu_find_output, normalize_warning_program,
};
pub use planner::{action_labels, contains_action, contains_predicate, predicate_labels};

pub fn argv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

pub fn path_arg(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
}

pub fn temp_output_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

pub fn rushfind_command() -> Command {
    let mut command = Command::new(cargo_bin("rfd"));
    command.env("RUSHFIND_WARNINGS", "off");
    command
}

pub fn rushfind_command_with_workers(workers: usize) -> Command {
    let mut command = rushfind_command();
    command.env("RUSHFIND_WORKERS", workers.to_string());
    command
}

#[cfg(unix)]
static NON_UTF8_TEMP_PATHS_SUPPORTED: OnceLock<bool> = OnceLock::new();
#[cfg(unix)]
static NON_UTF8_TEMP_PATHS_UNSUPPORTED_REPORTED: Once = Once::new();

#[cfg(unix)]
fn detect_non_utf8_temp_paths() -> bool {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(std::path::PathBuf::from(OsString::from_vec(
        b"probe-\xff".to_vec(),
    )));

    match fs::write(&path, []) {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => false,
        Err(error) => panic!("unexpected non-UTF-8 temp path probe failure: {error}"),
    }
}

#[cfg(unix)]
fn report_unsupported_non_utf8_temp_paths() {
    NON_UTF8_TEMP_PATHS_UNSUPPORTED_REPORTED.call_once(|| {
        eprintln!(
            "skipping non-UTF-8 filesystem test: temp filesystem rejects raw non-UTF-8 path bytes"
        );
    });
}

#[cfg(unix)]
pub fn supports_non_utf8_temp_paths() -> bool {
    if *NON_UTF8_TEMP_PATHS_SUPPORTED.get_or_init(detect_non_utf8_temp_paths) {
        true
    } else {
        report_unsupported_non_utf8_temp_paths();
        false
    }
}

pub fn lines(bytes: &[u8]) -> BTreeSet<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| line.to_string())
        .collect()
}

pub fn newline_records(bytes: &[u8]) -> BTreeSet<Vec<u8>> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
        .map(|record| record.to_vec())
        .collect()
}

pub fn nul_records(bytes: &[u8]) -> BTreeSet<Vec<u8>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| record.to_vec())
        .collect()
}

pub fn cargo_bin_output_with_timeout(
    args: &[OsString],
    workers: usize,
    timeout: Duration,
) -> Output {
    cargo_bin_output_with_env_timeout(args, workers, &[], timeout)
}

pub fn cargo_bin_output_with_env_timeout(
    args: &[OsString],
    workers: usize,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> Output {
    let mut command = rushfind_command_with_workers(workers);
    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("rfd did not exit within {:?}", timeout);
        }
    }
}

pub fn cargo_bin_output_with_input_timeout(
    args: &[OsString],
    workers: usize,
    input: &[u8],
    timeout: Duration,
) -> Output {
    let mut command = rushfind_command_with_workers(workers);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    if !input.is_empty() {
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(child.stdin.take());

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("rfd did not exit within {:?}", timeout);
        }
    }
}

pub fn cargo_bin_output_with_env_and_input_timeout(
    args: &[OsString],
    workers: usize,
    envs: &[(&str, &str)],
    input: &[u8],
    timeout: Duration,
) -> Output {
    let mut command = rushfind_command_with_workers(workers);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    if !input.is_empty() {
        child.stdin.as_mut().unwrap().write_all(input).unwrap();
    }
    drop(child.stdin.take());

    match child.wait_timeout(timeout).unwrap() {
        Some(_) => child.wait_with_output().unwrap(),
        None => {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("rfd did not exit within {:?}", timeout);
        }
    }
}

pub fn first_available_locale(candidates: &[&str]) -> Option<String> {
    let output = Command::new("locale").arg("-a").output().ok()?;
    let available = String::from_utf8_lossy(&output.stdout);
    candidates.iter().find_map(|candidate| {
        available
            .lines()
            .find(|line| *line == *candidate)
            .map(str::to_string)
    })
}

pub fn resolved_messages_locale(locale: &str) -> Option<String> {
    let output = Command::new("locale")
        .env("LANG", "C")
        .env("LC_MESSAGES", locale)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        line.strip_prefix("LC_MESSAGES=")
            .map(|value| value.trim_matches('"').to_string())
    })
}

#[cfg(unix)]
pub fn locale_affirmative_accepts(locale: &str, reply: &str) -> bool {
    let output = match Command::new("locale")
        .env("LANG", "C")
        .env("LC_MESSAGES", locale)
        .args(["yesstr", "yesexpr"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return false,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let yesstr = lines.next().unwrap_or("").trim();
    let yesexpr = lines.next().unwrap_or("").trim();

    if !yesstr.is_empty()
        && yesstr
            .to_ascii_lowercase()
            .contains(&reply.to_ascii_lowercase())
    {
        return true;
    }
    if yesexpr.is_empty() {
        return false;
    }

    let Ok(pattern) = CString::new(yesexpr) else {
        return false;
    };
    let Ok(reply) = CString::new(reply) else {
        return false;
    };

    locale_yesexpr_matches(&pattern, &reply)
}

#[cfg(windows)]
pub fn locale_affirmative_accepts(_locale: &str, _reply: &str) -> bool {
    false
}

#[cfg(all(unix, any(target_os = "solaris", target_os = "illumos")))]
fn locale_yesexpr_matches(pattern: &CString, reply: &CString) -> bool {
    unsafe { rushfind_regexec_match(pattern.as_ptr(), reply.as_ptr()) == 1 }
}

#[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
fn locale_yesexpr_matches(pattern: &CString, reply: &CString) -> bool {
    let mut regex = MaybeUninit::<libc::regex_t>::zeroed();
    let compile_status = unsafe {
        libc::regcomp(
            regex.as_mut_ptr(),
            pattern.as_ptr(),
            libc::REG_EXTENDED | libc::REG_NOSUB,
        )
    };
    if compile_status != 0 {
        return false;
    }

    let mut regex = unsafe { regex.assume_init() };
    let exec_status = unsafe { libc::regexec(&regex, reply.as_ptr(), 0, std::ptr::null_mut(), 0) };
    unsafe {
        libc::regfree(&mut regex);
    }

    exec_status == 0
}

pub fn existing_path_without_birth_time() -> Option<std::path::PathBuf> {
    for candidate in [
        "/proc/self/stat",
        "/proc/self/status",
        "/dev/null",
        "/dev/zero",
        "/dev/fd/0",
    ] {
        let path = Path::new(candidate);
        if !path.exists() {
            continue;
        }

        if matches!(read_birth_time(path, true), Ok(None)) {
            return Some(path.to_path_buf());
        }
    }

    None
}
