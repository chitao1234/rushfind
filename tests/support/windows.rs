#![cfg(windows)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use tempfile::TempDir;
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NONE_MAPPED, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GROUP_SECURITY_INFORMATION, LookupAccountSidW, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE,
};

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
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("probe.txt");
    fs::write(&path, b"probe").unwrap();
    file_security_descriptor(&path).is_ok()
}

pub(crate) fn file_owner_name(path: &std::path::Path) -> String {
    let descriptor = file_security_descriptor(path).unwrap();
    sid_to_account_name(descriptor.owner)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn file_owner_sid(path: &std::path::Path) -> String {
    let descriptor = file_security_descriptor(path).unwrap();
    sid_to_string(descriptor.owner).unwrap()
}

pub(crate) fn file_group_sid(path: &std::path::Path) -> String {
    let descriptor = file_security_descriptor(path).unwrap();
    sid_to_string(descriptor.group).unwrap()
}

struct FileSecurityDescriptor {
    owner: PSID,
    group: PSID,
    descriptor: PSECURITY_DESCRIPTOR,
}

impl Drop for FileSecurityDescriptor {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            unsafe {
                let _ = LocalFree(self.descriptor.cast());
            }
        }
    }
}

fn file_security_descriptor(path: &std::path::Path) -> io::Result<FileSecurityDescriptor> {
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut owner = null_mut();
    let mut group = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
            &mut owner,
            &mut group,
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }

    Ok(FileSecurityDescriptor {
        owner,
        group,
        descriptor,
    })
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut raw = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let owned = LocalWideString(raw);
    Ok(String::from_utf16_lossy(owned.as_slice()))
}

fn sid_to_account_name(sid: PSID) -> io::Result<OsString> {
    let mut name_len = 0u32;
    let mut domain_len = 0u32;
    let mut sid_use: SID_NAME_USE = 0;
    let ok = unsafe {
        LookupAccountSidW(
            null(),
            sid,
            null_mut(),
            &mut name_len,
            null_mut(),
            &mut domain_len,
            &mut sid_use,
        )
    };
    if ok != 0 {
        return Err(io::Error::other(
            "LookupAccountSidW unexpectedly succeeded without output buffers",
        ));
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == ERROR_INSUFFICIENT_BUFFER as i32 => {}
        Some(code) if code == ERROR_NONE_MAPPED as i32 => return Err(error),
        _ => return Err(error),
    }

    let mut name = vec![0u16; name_len as usize];
    let mut domain = vec![0u16; domain_len as usize];
    if unsafe {
        LookupAccountSidW(
            null(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_use,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    Ok(format_account_name(&domain, &name))
}

fn format_account_name(domain: &[u16], name: &[u16]) -> OsString {
    let domain = trim_wide(domain);
    let name = trim_wide(name);
    if domain.is_empty() {
        return OsString::from_wide(name);
    }

    let mut combined = Vec::with_capacity(domain.len() + 1 + name.len());
    combined.extend_from_slice(domain);
    combined.push(b'\\' as u16);
    combined.extend_from_slice(name);
    OsString::from_wide(&combined)
}

fn trim_wide(value: &[u16]) -> &[u16] {
    let nul = value
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(value.len());
    &value[..nul]
}

struct LocalWideString(*mut u16);

impl LocalWideString {
    fn as_slice(&self) -> &[u16] {
        if self.0.is_null() {
            return &[];
        }

        let mut len = 0usize;
        unsafe {
            while *self.0.add(len) != 0 {
                len += 1;
            }
            std::slice::from_raw_parts(self.0, len)
        }
    }
}

impl Drop for LocalWideString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0.cast());
            }
        }
    }
}
