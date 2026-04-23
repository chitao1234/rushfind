#![cfg(windows)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

pub(crate) fn normalize_stdout_path(text: &str) -> String {
    text.replace('/', "\\")
}

pub(crate) fn escape_ls_rendered_path(text: &str) -> String {
    normalize_stdout_path(text).replace('\\', "\\\\")
}

pub(crate) fn symlink_creation_available() -> bool {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.txt");
    let link = root.path().join("link.txt");
    fs::write(&target, b"target").unwrap();
    if std::os::windows::fs::symlink_file(&target, &link).is_err() {
        return false;
    }
    fs::read_to_string(&link)
        .map(|content| content == "target")
        .unwrap_or(false)
}

pub(crate) fn directory_symlink_creation_available() -> bool {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("probe.txt"), b"probe").unwrap();
    if std::os::windows::fs::symlink_dir(&target, &link).is_err() {
        return false;
    }
    fs::read_dir(&link).is_ok() && fs::read(link.join("probe.txt")).is_ok()
}

pub(crate) fn write_arg_echo_script(prefix: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("echo-args.cmd");
    fs::write(
        &script,
        format!(
            "@echo off\r\n\
             :loop\r\n\
             if \"%~1\"==\"\" goto done\r\n\
             echo {prefix}%~1\r\n\
             shift\r\n\
             goto loop\r\n\
             :done\r\n"
        ),
    )
    .unwrap();
    (dir, script)
}

pub(crate) fn ownership_probe_available() -> bool {
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Write-Output RUSHFIND-POWERSHELL-READY",
        ])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("RUSHFIND-POWERSHELL-READY")
        })
        .unwrap_or(false)
}

pub(crate) fn file_owner_name(path: &std::path::Path) -> String {
    powershell_path_query(
        path,
        "$acl = Get-Acl -LiteralPath $env:RUSHFIND_TEST_PATH; \
         $acl.GetOwner([System.Security.Principal.NTAccount]).Value",
    )
}

pub(crate) fn file_owner_sid(path: &std::path::Path) -> String {
    powershell_path_query(
        path,
        "$acl = Get-Acl -LiteralPath $env:RUSHFIND_TEST_PATH; \
         $acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value",
    )
}

pub(crate) fn file_group_sid(path: &std::path::Path) -> String {
    powershell_path_query(
        path,
        "$acl = Get-Acl -LiteralPath $env:RUSHFIND_TEST_PATH; \
         $acl.GetGroup([System.Security.Principal.SecurityIdentifier]).Value",
    )
}

fn powershell_path_query(path: &std::path::Path, script: &str) -> String {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .env("RUSHFIND_TEST_PATH", path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .trim_end_matches('\r')
        .to_owned()
}
