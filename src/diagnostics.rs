use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub exit_code: i32,
    /// Operating-system error behind this diagnostic, when there is one. The
    /// traversal uses it to recognize the errors a directory entry race
    /// produces, which `-ignore_readdir_race` hides.
    raw_os_error: Option<i32>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            message: message.into(),
            exit_code,
            raw_os_error: None,
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(message, 1)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(message, 1)
    }

    pub(crate) fn with_raw_os_error(mut self, error: Option<i32>) -> Self {
        self.raw_os_error = error;
        self
    }

    /// A directory entry race: the entry was read but is gone (or stale) by the
    /// time it is examined. Permission and I/O failures are not races.
    pub(crate) fn is_readdir_race(&self) -> bool {
        let Some(code) = self.raw_os_error else {
            return false;
        };

        if code == libc::ENOENT {
            return true;
        }

        // Windows has no stale-handle errno, and its libc bindings do not
        // define ESTALE.
        #[cfg(unix)]
        {
            code == libc::ESTALE
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

pub(crate) fn failed_to_write(target: &str, error: impl fmt::Display) -> Diagnostic {
    Diagnostic::new(format!("failed to write {target}: {error}"), 1)
}

pub(crate) fn internal_unavailable(resource: &str) -> Diagnostic {
    Diagnostic::new(format!("internal error: {resource} is unavailable"), 1)
}

pub(crate) fn internal_error(message: &str) -> Diagnostic {
    Diagnostic::new(format!("internal error: {message}"), 1)
}

pub(crate) fn internal_poisoned(resource: &str) -> Diagnostic {
    Diagnostic::new(format!("internal error: {resource} was poisoned"), 1)
}

pub(crate) fn runtime_stderr_line(message: impl fmt::Display) -> Vec<u8> {
    format!("rfd: {message}\n").into_bytes()
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {}
