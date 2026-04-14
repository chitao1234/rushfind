use crate::diagnostics::Diagnostic;
use std::ffi::CString;
use std::ffi::OsStr;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

pub fn matches_pattern(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    let pattern = cstring_from_os(pattern, "pattern")?;
    let candidate = cstring_from_os(candidate, "candidate")?;

    let mut flags = 0;
    if pathname {
        flags |= libc::FNM_PATHNAME;
    }

    #[cfg(target_os = "linux")]
    if case_insensitive {
        flags |= libc::FNM_CASEFOLD;
    }

    let result = unsafe { libc::fnmatch(pattern.as_ptr(), candidate.as_ptr(), flags) };
    Ok(result == 0)
}

#[cfg(unix)]
fn cstring_from_os(value: &OsStr, label: &str) -> Result<CString, Diagnostic> {
    CString::new(value.as_bytes())
        .map_err(|_| Diagnostic::new(format!("{label} contains an interior NUL byte"), 1))
}

#[cfg(not(unix))]
fn cstring_from_os(value: &OsStr, label: &str) -> Result<CString, Diagnostic> {
    CString::new(value.to_string_lossy().into_owned())
        .map_err(|_| Diagnostic::new(format!("{label} contains an interior NUL byte"), 1))
}
