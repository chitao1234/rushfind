use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub exit_code: i32,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(message, 1)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(message, 1)
    }
}

pub(crate) fn failed_to_write(target: &str, error: impl fmt::Display) -> Diagnostic {
    Diagnostic::new(format!("failed to write {target}: {error}"), 1)
}

pub(crate) fn internal_unavailable(resource: &str) -> Diagnostic {
    Diagnostic::new(format!("internal error: {resource} is unavailable"), 1)
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
