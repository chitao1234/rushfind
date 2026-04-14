use crate::diagnostics::Diagnostic;
use std::ffi::CString;

pub fn matches_pattern(
    pattern: &str,
    candidate: &str,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    let pattern = CString::new(pattern)
        .map_err(|_| Diagnostic::new("pattern contains an interior NUL byte", 1))?;
    let candidate = CString::new(candidate)
        .map_err(|_| Diagnostic::new("candidate contains an interior NUL byte", 1))?;

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
