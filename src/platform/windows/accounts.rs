use crate::diagnostics::Diagnostic;
use crate::platform::accounts::AccountBackend;
use crate::platform::filesystem::PlatformPrincipalId;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NONE_MAPPED, LocalFree};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows_sys::Win32::Security::{LookupAccountNameW, LookupAccountSidW, PSID, SID_NAME_USE};

static WINDOWS_ACCOUNT_BACKEND: WindowsAccountBackend = WindowsAccountBackend;
static SID_BY_NAME_CACHE: OnceLock<Mutex<HashMap<OsString, Option<String>>>> = OnceLock::new();
static NAME_BY_SID_CACHE: OnceLock<Mutex<HashMap<String, Option<OsString>>>> = OnceLock::new();

pub(crate) fn backend() -> &'static dyn AccountBackend {
    &WINDOWS_ACCOUNT_BACKEND
}

struct WindowsAccountBackend;

impl AccountBackend for WindowsAccountBackend {
    fn resolve_user_principal(&self, raw: &OsStr) -> Result<PlatformPrincipalId, Diagnostic> {
        let name = raw.to_string_lossy().into_owned();
        match cached_sid_for_name(raw)? {
            Some(sid) => Ok(PlatformPrincipalId::Sid(sid)),
            None => Err(Diagnostic::new(
                format!("`{name}` is not the name of a known user"),
                1,
            )),
        }
    }

    fn resolve_group_principal(&self, raw: &OsStr) -> Result<PlatformPrincipalId, Diagnostic> {
        let name = raw.to_string_lossy().into_owned();
        match cached_sid_for_name(raw)? {
            Some(sid) => Ok(PlatformPrincipalId::Sid(sid)),
            None => Err(Diagnostic::new(
                format!("`{name}` is not the name of an existing group"),
                1,
            )),
        }
    }

    fn user_exists(&self, principal: &PlatformPrincipalId) -> Result<bool, Diagnostic> {
        cached_name_for_principal(principal).map(|name| name.is_some())
    }

    fn group_exists(&self, principal: &PlatformPrincipalId) -> Result<bool, Diagnostic> {
        cached_name_for_principal(principal).map(|name| name.is_some())
    }

    fn user_name(&self, principal: &PlatformPrincipalId) -> Result<Option<OsString>, Diagnostic> {
        cached_name_for_principal(principal)
    }

    fn group_name(&self, principal: &PlatformPrincipalId) -> Result<Option<OsString>, Diagnostic> {
        cached_name_for_principal(principal)
    }
}

fn cached_sid_for_name(raw: &OsStr) -> Result<Option<String>, Diagnostic> {
    let cache = SID_BY_NAME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache.lock().unwrap().get(raw).cloned() {
        return Ok(value);
    }

    let value = lookup_sid_for_name(raw)?;
    cache
        .lock()
        .unwrap()
        .insert(raw.to_os_string(), value.clone());
    Ok(value)
}

fn cached_name_for_principal(
    principal: &PlatformPrincipalId,
) -> Result<Option<OsString>, Diagnostic> {
    let PlatformPrincipalId::Sid(sid) = principal else {
        return Ok(None);
    };

    let cache = NAME_BY_SID_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(value) = cache.lock().unwrap().get(sid).cloned() {
        return Ok(value);
    }

    let value = lookup_name_for_sid(sid)?;
    cache.lock().unwrap().insert(sid.clone(), value.clone());
    Ok(value)
}

fn lookup_sid_for_name(raw: &OsStr) -> Result<Option<String>, Diagnostic> {
    let account_name = wide_null(raw);
    let mut sid_len = 0u32;
    let mut domain_len = 0u32;
    let mut sid_use = 0i32;
    let ok = unsafe {
        LookupAccountNameW(
            null(),
            account_name.as_ptr(),
            null_mut(),
            &mut sid_len,
            null_mut(),
            &mut domain_len,
            &mut sid_use,
        )
    };
    if ok != 0 {
        return Err(Diagnostic::new(
            format!(
                "internal error: LookupAccountNameW unexpectedly succeeded for `{}` without a SID buffer",
                raw.to_string_lossy()
            ),
            1,
        ));
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == ERROR_NONE_MAPPED as i32 => return Ok(None),
        Some(code) if code == ERROR_INSUFFICIENT_BUFFER as i32 => {}
        _ => {
            return Err(Diagnostic::new(
                format!(
                    "failed to look up account `{}`: {error}",
                    raw.to_string_lossy()
                ),
                1,
            ));
        }
    }

    let mut sid = vec![0u8; sid_len as usize];
    let mut domain = vec![0u16; domain_len as usize];
    if unsafe {
        LookupAccountNameW(
            null(),
            account_name.as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_use,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NONE_MAPPED as i32) {
            return Ok(None);
        }
        return Err(Diagnostic::new(
            format!(
                "failed to look up account `{}`: {error}",
                raw.to_string_lossy()
            ),
            1,
        ));
    }

    sid_to_string(sid.as_mut_ptr().cast())
        .map(Some)
        .map_err(|error| Diagnostic::new(error.to_string(), 1))
}

fn lookup_name_for_sid(sid: &str) -> Result<Option<OsString>, Diagnostic> {
    let sid_text = sid;
    let sid = OwnedLocalPointer::from_sid_string(sid_text).map_err(|error| {
        Diagnostic::new(format!("failed to parse SID `{sid_text}`: {error}"), 1)
    })?;

    let mut name_len = 0u32;
    let mut domain_len = 0u32;
    let mut sid_use: SID_NAME_USE = 0;
    let ok = unsafe {
        LookupAccountSidW(
            null(),
            sid.as_ptr(),
            null_mut(),
            &mut name_len,
            null_mut(),
            &mut domain_len,
            &mut sid_use,
        )
    };
    if ok != 0 {
        return Err(Diagnostic::new(
            format!(
                "internal error: LookupAccountSidW unexpectedly succeeded for `{sid_text}` without output buffers"
            ),
            1,
        ));
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == ERROR_NONE_MAPPED as i32 => return Ok(None),
        Some(code) if code == ERROR_INSUFFICIENT_BUFFER as i32 => {}
        _ => {
            return Err(Diagnostic::new(
                format!("failed to resolve SID `{sid_text}`: {error}"),
                1,
            ));
        }
    }

    let mut name = vec![0u16; name_len as usize];
    let mut domain = vec![0u16; domain_len as usize];
    if unsafe {
        LookupAccountSidW(
            null(),
            sid.as_ptr(),
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_use,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NONE_MAPPED as i32) {
            return Ok(None);
        }
        return Err(Diagnostic::new(
            format!("failed to resolve SID `{sid_text}`: {error}"),
            1,
        ));
    }

    Ok(Some(format_account_name(&domain, &name)))
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

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut raw = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let owned = OwnedLocalPointer::from_raw(raw.cast());
    Ok(String::from_utf16_lossy(trim_wide(owned.as_wide())))
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn trim_wide(value: &[u16]) -> &[u16] {
    let nul = value
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(value.len());
    &value[..nul]
}

struct OwnedLocalPointer<T>(*mut T);

impl<T> OwnedLocalPointer<T> {
    fn from_raw(ptr: *mut T) -> Self {
        Self(ptr)
    }
}

impl OwnedLocalPointer<std::ffi::c_void> {
    fn from_sid_string(value: &str) -> io::Result<Self> {
        let mut sid = null_mut();
        let value = value.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        if unsafe { ConvertStringSidToSidW(value.as_ptr(), &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self::from_raw(sid))
    }

    fn as_ptr(&self) -> PSID {
        self.0
    }
}

impl OwnedLocalPointer<u16> {
    fn as_wide(&self) -> &[u16] {
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

impl<T> Drop for OwnedLocalPointer<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0.cast());
            }
        }
    }
}
