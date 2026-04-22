use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::template::{BatchedExecAction, ExecBatchId};

#[derive(Debug, Clone, Copy)]
pub struct BatchLimit {
    max_bytes: usize,
}

impl BatchLimit {
    pub fn detect() -> Self {
        let arg_max = detect_arg_max();
        let env_bytes = std::env::vars_os()
            .map(|(key, value)| os_bytes_len(key.as_os_str()) + os_bytes_len(value.as_os_str()) + 2)
            .sum::<usize>();
        let safety_margin = 4096;

        Self {
            max_bytes: arg_max
                .saturating_sub(env_bytes)
                .saturating_sub(safety_margin),
        }
    }

    pub fn for_tests(max_bytes: usize) -> Self {
        Self { max_bytes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyBatch {
    pub spec: BatchedExecAction,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExecBatchKey {
    pub id: ExecBatchId,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PendingBatch {
    pub spec: BatchedExecAction,
    pub paths: Vec<PathBuf>,
    used_bytes: usize,
    fixed_bytes: usize,
    limit: BatchLimit,
}

impl PendingBatch {
    pub fn new(spec: BatchedExecAction, limit: BatchLimit, fixed_bytes: usize) -> Self {
        Self {
            spec,
            paths: Vec::new(),
            used_bytes: fixed_bytes,
            fixed_bytes,
            limit,
        }
    }

    pub fn push(&mut self, path: &Path) -> Result<Option<ReadyBatch>, Diagnostic> {
        let path_bytes = crate::exec::template::batched_path_cost(&self.spec, path);
        if self.paths.is_empty() && self.used_bytes + path_bytes > self.limit.max_bytes {
            return Err(Diagnostic::new(
                format!(
                    "{}: path is too large for `{}`",
                    path.display(),
                    self.spec.batch_flag()
                ),
                1,
            ));
        }

        if !self.paths.is_empty() && self.used_bytes + path_bytes > self.limit.max_bytes {
            let flushed = self.take_ready();
            self.paths.push(path.to_path_buf());
            self.used_bytes += path_bytes;
            return Ok(Some(flushed));
        }

        self.paths.push(path.to_path_buf());
        self.used_bytes += path_bytes;
        Ok(None)
    }

    pub fn would_overflow(&self, path: &Path) -> bool {
        self.used_bytes + crate::exec::template::batched_path_cost(&self.spec, path)
            > self.limit.max_bytes
    }

    pub fn take_ready(&mut self) -> ReadyBatch {
        let paths = std::mem::take(&mut self.paths);
        self.used_bytes = self.fixed_bytes;
        ReadyBatch {
            spec: self.spec.clone(),
            paths,
        }
    }
}

pub(crate) fn fixed_batch_cost(spec: &BatchedExecAction) -> usize {
    spec.argv_prefix
        .iter()
        .map(|arg| os_bytes_len(arg.as_os_str()) + 1)
        .sum()
}

fn os_bytes_len(value: &OsStr) -> usize {
    crate::platform::path::encoded_bytes(value).len()
}

#[cfg(unix)]
fn detect_arg_max() -> usize {
    let arg_max = unsafe { libc::sysconf(libc::_SC_ARG_MAX) };
    usize::try_from(arg_max.max(4096)).unwrap_or(4096)
}

#[cfg(windows)]
fn detect_arg_max() -> usize {
    32_767
}
