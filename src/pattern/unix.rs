use crate::diagnostics::Diagnostic;
use crate::pattern::{GlobCaseMode, GlobSlashMode};
use std::ffi::{CString, OsStr};

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
        flags |= libc::FNM_CASEFOLD;
    }

    Ok(unsafe { libc::fnmatch(pattern.as_ptr(), candidate.as_ptr(), flags) == 0 })
}
