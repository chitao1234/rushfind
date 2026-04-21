use crate::diagnostics::Diagnostic;
use crate::pattern::{GlobCaseMode, GlobSlashMode};
use std::ffi::{CString, OsStr};

#[cfg(target_os = "macos")]
const fn fnmatch_casefold_flag() -> libc::c_int {
    0
}

#[cfg(not(target_os = "macos"))]
const fn fnmatch_casefold_flag() -> libc::c_int {
    libc::FNM_CASEFOLD
}

pub(super) fn fnmatch_fallback(
    pattern: &[u8],
    candidate: &OsStr,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
) -> Result<bool, Diagnostic> {
    let pattern = CString::new(pattern)
        .map_err(|_| Diagnostic::new("glob pattern contains an interior NUL byte", 1))?;
    let candidate = CString::new(candidate.as_encoded_bytes())
        .map_err(|_| Diagnostic::new("candidate contains an interior NUL byte", 1))?;

    let mut flags = 0;
    if slash_mode == GlobSlashMode::Pathname {
        flags |= libc::FNM_PATHNAME;
    }
    if case_mode == GlobCaseMode::Insensitive {
        flags |= fnmatch_casefold_flag();
    }

    Ok(unsafe { libc::fnmatch(pattern.as_ptr(), candidate.as_ptr(), flags) == 0 })
}
