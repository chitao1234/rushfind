#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use std::fs;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use tempfile::tempdir;

/// `find . -print | head -1` relies on the writer dying from `SIGPIPE` once the
/// reader goes away. Rust ignores `SIGPIPE` by default, so `rfd` has to restore
/// the default disposition explicitly; otherwise the pipeline reports a broken
/// pipe as an `rfd` runtime error.
#[test]
fn closed_stdout_pipe_terminates_rfd_with_sigpipe() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "content\n").unwrap();

    let (read_end, write_end): (RawFd, RawFd) = {
        let mut ends = [0 as RawFd; 2];
        // SAFETY: `ends` is a valid two-element array for `pipe(2)` to fill in.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        (ends[0], ends[1])
    };
    // SAFETY: the read end is only closed here and never used again.
    assert_eq!(unsafe { libc::close(read_end) }, 0);
    // SAFETY: `write_end` is an open descriptor that `Stdio` takes ownership of.
    let stdout = unsafe { Stdio::from_raw_fd(write_end) };

    let status = Command::new(cargo_bin("rfd"))
        .arg(root.path())
        .arg("-print")
        .stdout(stdout)
        .stderr(Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        status.signal(),
        Some(libc::SIGPIPE),
        "expected termination by SIGPIPE, got {status:?}"
    );
}
