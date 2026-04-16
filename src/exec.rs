use crate::diagnostics::Diagnostic;
use crate::eval::ActionSink;
use crate::output::StdoutSink;
use crate::planner::RuntimeAction;
use libc::_SC_ARG_MAX;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub type ExecBatchId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecTemplateSegment {
    Literal(OsString),
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmediateExecAction {
    pub argv: Vec<Vec<ExecTemplateSegment>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchedExecAction {
    pub id: ExecBatchId,
    pub argv_prefix: Vec<OsString>,
}

#[derive(Debug, Clone, Copy)]
pub struct BatchLimit {
    max_bytes: usize,
}

impl BatchLimit {
    pub fn detect() -> Self {
        let arg_max = unsafe { libc::sysconf(_SC_ARG_MAX) };
        let arg_max = usize::try_from(arg_max.max(4096)).unwrap_or(4096);
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
        let path_bytes = batch_path_cost(path);
        if self.paths.is_empty() && self.used_bytes + path_bytes > self.limit.max_bytes {
            return Err(Diagnostic::new(
                format!("{}: path is too large for `-exec ... +`", path.display()),
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
        self.used_bytes + batch_path_cost(path) > self.limit.max_bytes
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

pub struct OrderedActionSink<'a, W: std::io::Write, E: std::io::Write> {
    output: StdoutSink<'a, W>,
    stderr: &'a mut E,
    pending: BTreeMap<ExecBatchId, PendingBatch>,
    batch_limit: BatchLimit,
    had_batch_failures: bool,
}

impl<'a, W: std::io::Write, E: std::io::Write> OrderedActionSink<'a, W, E> {
    pub fn new(stdout: &'a mut W, stderr: &'a mut E) -> Self {
        Self {
            output: StdoutSink::new(stdout),
            stderr,
            pending: BTreeMap::new(),
            batch_limit: BatchLimit::detect(),
            had_batch_failures: false,
        }
    }

    fn enqueue(&mut self, spec: &BatchedExecAction, path: &Path) -> Result<(), Diagnostic> {
        let ready = {
            let batch = self.pending.entry(spec.id).or_insert_with(|| {
                PendingBatch::new(spec.clone(), self.batch_limit, fixed_batch_cost(spec))
            });

            if !batch.paths.is_empty() && batch.would_overflow(path) {
                Some(batch.take_ready())
            } else {
                None
            }
        };

        if let Some(ready) = ready {
            if !run_ready_batch(&ready, self.stderr)? {
                self.had_batch_failures = true;
            }
        }

        let push_result = {
            let batch = self
                .pending
                .get_mut(&spec.id)
                .expect("pending batch must exist");
            batch.push(path)
        };

        match push_result {
            Ok(Some(ready)) => {
                if !run_ready_batch(&ready, self.stderr)? {
                    self.had_batch_failures = true;
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.write_diagnostic(&format!("findoxide: {error}"))?;
                self.had_batch_failures = true;
            }
        }

        Ok(())
    }

    pub fn write_diagnostic(&mut self, message: &str) -> Result<(), Diagnostic> {
        writeln!(self.stderr, "{message}")
            .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))
    }

    pub fn flush(&mut self) -> Result<bool, Diagnostic> {
        let pending = std::mem::take(&mut self.pending);
        for (_, batch) in pending {
            if batch.paths.is_empty() {
                continue;
            }

            let ready = ReadyBatch {
                spec: batch.spec,
                paths: batch.paths,
            };
            if !run_ready_batch(&ready, self.stderr)? {
                self.had_batch_failures = true;
            }
        }

        Ok(self.had_batch_failures)
    }
}

impl<W: std::io::Write, E: std::io::Write> ActionSink for OrderedActionSink<'_, W, E> {
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path) -> Result<bool, Diagnostic> {
        match action {
            RuntimeAction::Output(_) => self.output.dispatch(action, path),
            RuntimeAction::ExecImmediate(spec) => run_immediate_ordered(spec, path, self.stderr),
            RuntimeAction::ExecBatched(spec) => {
                self.enqueue(spec, path)?;
                Ok(true)
            }
        }
    }
}

pub fn compile_immediate_exec(argv: &[OsString]) -> ImmediateExecAction {
    ImmediateExecAction {
        argv: argv
            .iter()
            .map(|arg| compile_segments(arg.as_os_str()))
            .collect(),
    }
}

pub fn compile_batched_exec(
    id: ExecBatchId,
    argv: &[OsString],
) -> Result<BatchedExecAction, Diagnostic> {
    const BATCH_PLACEHOLDER_ERROR: &str =
        "`-exec ... +` requires exactly one standalone `{}` as the final command argument";

    let placeholder_indexes = argv
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| is_standalone_placeholder(arg.as_os_str()).then_some(index))
        .collect::<Vec<_>>();

    let Some(&placeholder_index) = placeholder_indexes.first() else {
        return Err(Diagnostic::parse(BATCH_PLACEHOLDER_ERROR));
    };

    if placeholder_indexes.len() != 1 || placeholder_index + 1 != argv.len() {
        return Err(Diagnostic::parse(BATCH_PLACEHOLDER_ERROR));
    }

    if argv.iter().any(|arg| {
        !is_standalone_placeholder(arg.as_os_str()) && contains_placeholder(arg.as_os_str())
    }) {
        return Err(Diagnostic::parse(BATCH_PLACEHOLDER_ERROR));
    }

    Ok(BatchedExecAction {
        id,
        argv_prefix: argv[..placeholder_index].to_vec(),
    })
}

fn compile_segments(arg: &OsStr) -> Vec<ExecTemplateSegment> {
    let bytes = arg.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while index + 1 < bytes.len() {
        if bytes[index] == b'{' && bytes[index + 1] == b'}' {
            if start < index {
                segments.push(ExecTemplateSegment::Literal(os_string_from_bytes(
                    &bytes[start..index],
                )));
            }
            segments.push(ExecTemplateSegment::Path);
            index += 2;
            start = index;
            continue;
        }
        index += 1;
    }

    if start < bytes.len() || segments.is_empty() {
        segments.push(ExecTemplateSegment::Literal(os_string_from_bytes(
            &bytes[start..],
        )));
    }

    segments
}

fn is_standalone_placeholder(arg: &OsStr) -> bool {
    arg.as_bytes() == b"{}"
}

fn contains_placeholder(arg: &OsStr) -> bool {
    arg.as_bytes().windows(2).any(|window| window == b"{}")
}

fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from_vec(bytes.to_vec())
}

pub fn render_immediate_argv(spec: &ImmediateExecAction, path: &Path) -> Vec<OsString> {
    let path_bytes = path.as_os_str().as_bytes();

    spec.argv
        .iter()
        .map(|template| {
            let mut rendered = Vec::new();
            for segment in template {
                match segment {
                    ExecTemplateSegment::Literal(literal) => {
                        rendered.extend_from_slice(literal.as_os_str().as_bytes());
                    }
                    ExecTemplateSegment::Path => rendered.extend_from_slice(path_bytes),
                }
            }
            OsString::from_vec(rendered)
        })
        .collect()
}

pub fn build_batched_argv(spec: &BatchedExecAction, paths: &[PathBuf]) -> Vec<OsString> {
    let mut argv = spec.argv_prefix.clone();
    argv.extend(paths.iter().map(|path| path.as_os_str().to_os_string()));
    argv
}

fn run_immediate_ordered<E: std::io::Write>(
    spec: &ImmediateExecAction,
    path: &Path,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    let argv = render_immediate_argv(spec, path);
    let Some(program) = argv.first() else {
        return Err(Diagnostic::new(
            "internal error: immediate exec action missing command",
            1,
        ));
    };

    let mut command = Command::new(program);
    command.args(&argv[1..]);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    match command.status() {
        Ok(status) => Ok(status.success()),
        Err(error) => {
            writeln!(stderr, "findoxide: {}: {error}", program.to_string_lossy()).map_err(
                |io_error| Diagnostic::new(format!("failed to write stderr: {io_error}"), 1),
            )?;
            Ok(false)
        }
    }
}

fn run_ready_batch<E: std::io::Write>(
    ready: &ReadyBatch,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    run_ordered_batch(build_batched_argv(&ready.spec, &ready.paths), stderr)
}

fn run_ordered_batch<E: std::io::Write>(
    argv: Vec<OsString>,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    let Some(program) = argv.first() else {
        return Err(Diagnostic::new(
            "internal error: batched exec action missing command",
            1,
        ));
    };

    let mut command = Command::new(program);
    command.args(&argv[1..]);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    match command.status() {
        Ok(status) => Ok(status.success()),
        Err(error) => {
            writeln!(stderr, "findoxide: {}: {error}", program.to_string_lossy()).map_err(
                |io_error| Diagnostic::new(format!("failed to write stderr: {io_error}"), 1),
            )?;
            Ok(false)
        }
    }
}

fn fixed_batch_cost(spec: &BatchedExecAction) -> usize {
    spec.argv_prefix
        .iter()
        .map(|arg| os_bytes_len(arg.as_os_str()) + 1)
        .sum()
}

fn batch_path_cost(path: &Path) -> usize {
    os_bytes_len(path.as_os_str()) + 1
}

fn os_bytes_len(value: &OsStr) -> usize {
    value.as_bytes().len()
}

#[cfg(test)]
mod tests {
    use super::{BatchLimit, PendingBatch, build_batched_argv, compile_batched_exec};
    use std::path::PathBuf;

    #[test]
    fn batch_sizer_flushes_before_crossing_the_limit() {
        let spec = compile_batched_exec(7, &["echo".into(), "{}".into()]).unwrap();
        let mut batch = PendingBatch::new(spec, BatchLimit::for_tests(16), 0);

        assert_eq!(batch.push("aa".as_ref()).unwrap(), None);
        assert_eq!(batch.push("bbbb".as_ref()).unwrap(), None);
        let flushed = batch
            .push("cccccccc".as_ref())
            .unwrap()
            .expect("expected flush");

        assert_eq!(
            flushed.paths,
            vec![PathBuf::from("aa"), PathBuf::from("bbbb")]
        );
        assert_eq!(batch.paths, vec![PathBuf::from("cccccccc")]);
    }

    #[test]
    fn render_batched_argv_appends_paths_after_the_fixed_prefix() {
        let spec =
            compile_batched_exec(3, &["printf".into(), "%s\\n".into(), "{}".into()]).unwrap();
        let argv = build_batched_argv(&spec, &[PathBuf::from("a"), PathBuf::from("b")]);

        assert_eq!(argv, vec!["printf", "%s\\n", "a", "b"]);
    }
}
