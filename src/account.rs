use crate::diagnostics::Diagnostic;
use std::collections::HashMap;
use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::{Mutex, OnceLock};

static USER_EXISTS_CACHE: OnceLock<Mutex<HashMap<u32, bool>>> = OnceLock::new();
static GROUP_EXISTS_CACHE: OnceLock<Mutex<HashMap<u32, bool>>> = OnceLock::new();
static USER_NAME_CACHE: OnceLock<Mutex<HashMap<u32, Option<OsString>>>> = OnceLock::new();
static GROUP_NAME_CACHE: OnceLock<Mutex<HashMap<u32, Option<OsString>>>> = OnceLock::new();

pub fn resolve_user_id(raw: &OsStr) -> Result<u32, Diagnostic> {
    if let Some(uid) = parse_decimal_id(raw) {
        return Ok(uid);
    }

    let name = raw.to_string_lossy().into_owned();
    match lookup_user_by_name(raw)? {
        Some(uid) => Ok(uid),
        None => Err(Diagnostic::new(
            format!("`{name}` is not the name of a known user"),
            1,
        )),
    }
}

pub fn resolve_group_id(raw: &OsStr) -> Result<u32, Diagnostic> {
    if let Some(gid) = parse_decimal_id(raw) {
        return Ok(gid);
    }

    let name = raw.to_string_lossy().into_owned();
    match lookup_group_by_name(raw)? {
        Some(gid) => Ok(gid),
        None => Err(Diagnostic::new(
            format!("`{name}` is not the name of an existing group"),
            1,
        )),
    }
}

pub fn user_exists(uid: u32) -> Result<bool, Diagnostic> {
    cached_exists(&USER_EXISTS_CACHE, uid, lookup_user_by_id)
}

pub fn group_exists(gid: u32) -> Result<bool, Diagnostic> {
    cached_exists(&GROUP_EXISTS_CACHE, gid, lookup_group_by_id)
}

pub fn user_name(uid: u32) -> Result<Option<OsString>, Diagnostic> {
    cached_value(&USER_NAME_CACHE, uid, lookup_user_name_by_id)
}

pub fn group_name(gid: u32) -> Result<Option<OsString>, Diagnostic> {
    cached_value(&GROUP_NAME_CACHE, gid, lookup_group_name_by_id)
}

fn parse_decimal_id(raw: &OsStr) -> Option<u32> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    std::str::from_utf8(bytes).ok()?.parse::<u32>().ok()
}

fn cached_exists(
    cache: &'static OnceLock<Mutex<HashMap<u32, bool>>>,
    id: u32,
    lookup: fn(u32) -> Result<bool, Diagnostic>,
) -> Result<bool, Diagnostic> {
    cached_value(cache, id, lookup)
}

fn cached_value<T: Clone>(
    cache: &'static OnceLock<Mutex<HashMap<u32, T>>>,
    id: u32,
    lookup: fn(u32) -> Result<T, Diagnostic>,
) -> Result<T, Diagnostic> {
    let cache = cache.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache.lock().unwrap().get(&id).cloned() {
        return Ok(value);
    }

    let value = lookup(id)?;
    cache.lock().unwrap().insert(id, value.clone());
    Ok(value)
}

fn lookup_user_by_name(raw: &OsStr) -> Result<Option<u32>, Diagnostic> {
    let name = c_string(raw, "user name")?;
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETPW_R_SIZE_MAX)];

    loop {
        let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                &mut passwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok((!result.is_null()).then_some(passwd.pw_uid as u32));
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(None);
        }

        return Err(nss_error("failed to look up user by name", status));
    }
}

fn lookup_group_by_name(raw: &OsStr) -> Result<Option<u32>, Diagnostic> {
    let name = c_string(raw, "group name")?;
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETGR_R_SIZE_MAX)];

    loop {
        let mut group = unsafe { std::mem::zeroed::<libc::group>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getgrnam_r(
                name.as_ptr(),
                &mut group,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok((!result.is_null()).then_some(group.gr_gid as u32));
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(None);
        }

        return Err(nss_error("failed to look up group by name", status));
    }
}

fn lookup_user_by_id(uid: u32) -> Result<bool, Diagnostic> {
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETPW_R_SIZE_MAX)];

    loop {
        let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid as libc::uid_t,
                &mut passwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok(!result.is_null());
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(false);
        }

        return Err(nss_error("failed to look up user by id", status));
    }
}

fn lookup_group_by_id(gid: u32) -> Result<bool, Diagnostic> {
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETGR_R_SIZE_MAX)];

    loop {
        let mut group = unsafe { std::mem::zeroed::<libc::group>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getgrgid_r(
                gid as libc::gid_t,
                &mut group,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok(!result.is_null());
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(false);
        }

        return Err(nss_error("failed to look up group by id", status));
    }
}

fn lookup_user_name_by_id(uid: u32) -> Result<Option<OsString>, Diagnostic> {
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETPW_R_SIZE_MAX)];

    loop {
        let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid as libc::uid_t,
                &mut passwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok((!result.is_null()).then(|| unsafe {
                OsString::from_vec(std::ffi::CStr::from_ptr(passwd.pw_name).to_bytes().to_vec())
            }));
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(None);
        }

        return Err(nss_error("failed to look up user name by id", status));
    }
}

fn lookup_group_name_by_id(gid: u32) -> Result<Option<OsString>, Diagnostic> {
    let mut buffer = vec![0u8; initial_buffer_size(libc::_SC_GETGR_R_SIZE_MAX)];

    loop {
        let mut group = unsafe { std::mem::zeroed::<libc::group>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getgrgid_r(
                gid as libc::gid_t,
                &mut group,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == 0 {
            return Ok((!result.is_null()).then(|| unsafe {
                OsString::from_vec(std::ffi::CStr::from_ptr(group.gr_name).to_bytes().to_vec())
            }));
        }
        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if is_not_found_status(status) {
            return Ok(None);
        }

        return Err(nss_error("failed to look up group name by id", status));
    }
}

fn c_string(raw: &OsStr, label: &str) -> Result<CString, Diagnostic> {
    CString::new(raw.as_bytes())
        .map_err(|_| Diagnostic::new(format!("{label} contains an interior NUL byte"), 1))
}

fn initial_buffer_size(kind: libc::c_int) -> usize {
    let reported = unsafe { libc::sysconf(kind) };
    if reported > 0 {
        reported as usize
    } else {
        16 * 1024
    }
}

fn nss_error(prefix: &str, code: i32) -> Diagnostic {
    Diagnostic::new(
        format!("{prefix}: {}", io::Error::from_raw_os_error(code)),
        1,
    )
}

fn is_not_found_status(status: i32) -> bool {
    matches!(
        status,
        libc::ENOENT | libc::ESRCH | libc::EBADF | libc::EPERM
    )
}
