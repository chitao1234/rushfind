use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, ActionSink, RuntimeStatus};
use crate::follow::FollowMode;
use crate::output::{BrokerMessage, StdoutSink, render_runtime_action_bytes};
use crate::planner::RuntimeAction;
use crossbeam_channel::Sender;
use libc::_SC_ARG_MAX;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Seek, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub type ExecBatchId = u32;
const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;

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
    had_action_failures: bool,
}

impl<'a, W: std::io::Write, E: std::io::Write> OrderedActionSink<'a, W, E> {
    pub fn new(stdout: &'a mut W, stderr: &'a mut E) -> Self {
        Self {
            output: StdoutSink::new(stdout),
            stderr,
            pending: BTreeMap::new(),
            batch_limit: BatchLimit::detect(),
            had_action_failures: false,
        }
    }

    fn enqueue(
        &mut self,
        spec: &BatchedExecAction,
        path: &Path,
    ) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = RuntimeStatus::default();
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
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
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
                    self.had_action_failures = true;
                    status = status.merge(RuntimeStatus::action_failure());
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.write_diagnostic(&format!("findoxide: {error}"))?;
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    pub fn write_diagnostic(&mut self, message: &str) -> Result<(), Diagnostic> {
        writeln!(self.stderr, "{message}")
            .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))
    }

    pub fn flush(&mut self) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = if self.had_action_failures {
            RuntimeStatus::action_failure()
        } else {
            RuntimeStatus::default()
        };
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
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }
}

impl<W: std::io::Write, E: std::io::Write> ActionSink for OrderedActionSink<'_, W, E> {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<ActionOutcome, Diagnostic> {
        match action {
            RuntimeAction::Output(_) | RuntimeAction::Printf(_) => {
                self.output.dispatch(action, entry, follow_mode)
            }
            RuntimeAction::Quit => Ok(ActionOutcome::quit()),
            RuntimeAction::ExecImmediate(spec) => {
                run_immediate_ordered(spec, entry.path.as_path(), self.stderr).map(action_success)
            }
            RuntimeAction::ExecBatched(spec) => Ok(ActionOutcome {
                matched: true,
                status: self.enqueue(spec, entry.path.as_path())?,
            }),
            RuntimeAction::Delete => match delete_path(entry.path.as_path()) {
                Ok(result) => Ok(action_success(result)),
                Err(error) => {
                    self.write_diagnostic(&format!("findoxide: {}", error.message))?;
                    self.had_action_failures = true;
                    Ok(action_failure(false))
                }
            },
        }
    }
}

#[derive(Clone)]
pub struct ParallelActionSink {
    broker: Sender<BrokerMessage>,
    shared: Arc<ParallelExecShared>,
}

struct ParallelExecShared {
    pending: Mutex<BTreeMap<ExecBatchId, PendingBatch>>,
    batch_limit: BatchLimit,
    had_action_failures: AtomicBool,
    spill_threshold: usize,
}

impl ParallelActionSink {
    pub fn new(broker: Sender<BrokerMessage>, _workers: usize) -> Result<Self, Diagnostic> {
        Ok(Self {
            broker,
            shared: Arc::new(ParallelExecShared {
                pending: Mutex::new(BTreeMap::new()),
                batch_limit: BatchLimit::detect(),
                had_action_failures: AtomicBool::new(false),
                spill_threshold: DEFAULT_SPILL_THRESHOLD,
            }),
        })
    }

    pub fn flush_all(&self) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = if self.shared.had_action_failures.load(Ordering::SeqCst) {
            RuntimeStatus::action_failure()
        } else {
            RuntimeStatus::default()
        };
        let pending = {
            let mut pending = self.shared.pending.lock().map_err(|_| {
                Diagnostic::new("internal error: parallel exec batch state was poisoned", 1)
            })?;
            std::mem::take(&mut *pending)
        };

        for (_, batch) in pending {
            if batch.paths.is_empty() {
                continue;
            }

            let ready = ReadyBatch {
                spec: batch.spec,
                paths: batch.paths,
            };
            if !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)? {
                self.mark_action_failure();
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    fn enqueue(&self, spec: &BatchedExecAction, path: &Path) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = RuntimeStatus::default();
        let (ready, push_result) = {
            let mut pending = self.shared.pending.lock().map_err(|_| {
                Diagnostic::new("internal error: parallel exec batch state was poisoned", 1)
            })?;
            let batch = pending.entry(spec.id).or_insert_with(|| {
                PendingBatch::new(
                    spec.clone(),
                    self.shared.batch_limit,
                    fixed_batch_cost(spec),
                )
            });

            let ready = if !batch.paths.is_empty() && batch.would_overflow(path) {
                Some(batch.take_ready())
            } else {
                None
            };
            let push_result = batch.push(path);
            (ready, push_result)
        };

        if let Some(ready) = ready {
            if !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)? {
                self.mark_action_failure();
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        match push_result {
            Ok(Some(ready)) => {
                if !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)? {
                    self.mark_action_failure();
                    status = status.merge(RuntimeStatus::action_failure());
                }
            }
            Ok(None) => {}
            Err(error) => {
                send_broker_message(
                    &self.broker,
                    BrokerMessage::Stderr(format!("findoxide: {error}\n").into_bytes()),
                )?;
                self.mark_action_failure();
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    fn mark_action_failure(&self) {
        self.shared
            .had_action_failures
            .store(true, Ordering::SeqCst);
    }

    fn execute_action(
        &self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<ActionOutcome, Diagnostic> {
        match action {
            RuntimeAction::Output(_) | RuntimeAction::Printf(_) => {
                send_broker_message(
                    &self.broker,
                    BrokerMessage::Stdout(render_runtime_action_bytes(action, entry, follow_mode)?),
                )?;
                Ok(ActionOutcome::matched_true())
            }
            RuntimeAction::Quit => Ok(ActionOutcome::quit()),
            RuntimeAction::ExecImmediate(spec) => run_immediate_parallel(
                spec,
                entry.path.as_path(),
                &self.broker,
                self.shared.spill_threshold,
            )
            .map(action_success),
            RuntimeAction::ExecBatched(spec) => Ok(ActionOutcome {
                matched: true,
                status: self.enqueue(spec, entry.path.as_path())?,
            }),
            RuntimeAction::Delete => match delete_path(entry.path.as_path()) {
                Ok(result) => Ok(action_success(result)),
                Err(error) => {
                    send_broker_message(
                        &self.broker,
                        BrokerMessage::Stderr(
                            format!("findoxide: {}\n", error.message).into_bytes(),
                        ),
                    )?;
                    self.mark_action_failure();
                    Ok(action_failure(false))
                }
            },
        }
    }
}

impl ActionSink for ParallelActionSink {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<ActionOutcome, Diagnostic> {
        self.execute_action(action, entry, follow_mode)
    }
}

fn action_success(matched: bool) -> ActionOutcome {
    ActionOutcome {
        matched,
        status: RuntimeStatus::default(),
    }
}

fn action_failure(matched: bool) -> ActionOutcome {
    ActionOutcome {
        matched,
        status: RuntimeStatus::action_failure(),
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

pub(crate) fn delete_path(path: &Path) -> Result<bool, Diagnostic> {
    let file_type = fs::symlink_metadata(path)
        .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?
        .file_type();

    let result = if file_type.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(()) => Ok(true),
        Err(error) => Err(Diagnostic::new(format!("{}: {error}", path.display()), 1)),
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

pub(crate) fn run_immediate_parallel(
    spec: &ImmediateExecAction,
    path: &Path,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    run_parallel_command(render_immediate_argv(spec, path), broker, spill_threshold)
}

pub(crate) fn run_parallel_ready_batch(
    ready: &ReadyBatch,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    run_parallel_command(
        build_batched_argv(&ready.spec, &ready.paths),
        broker,
        spill_threshold,
    )
}

fn run_parallel_command(
    argv: Vec<OsString>,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    let Some(program) = argv.first() else {
        return Err(Diagnostic::new(
            "internal error: exec action missing command",
            1,
        ));
    };

    let mut command = Command::new(program);
    command.args(&argv[1..]);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            send_broker_message(
                broker,
                BrokerMessage::Stderr(
                    format!("findoxide: {}: {error}\n", program.to_string_lossy()).into_bytes(),
                ),
            )?;
            return Ok(false);
        }
    };

    let stdout = child
        .stdout
        .take()
        .expect("stdout is piped for parallel exec children");
    let stderr = child
        .stderr
        .take()
        .expect("stderr is piped for parallel exec children");

    let stdout_thread = std::thread::spawn(move || read_child_pipe(stdout, spill_threshold));
    let stderr_thread = std::thread::spawn(move || read_child_pipe(stderr, spill_threshold));
    let status = child
        .wait()
        .map_err(|error| Diagnostic::new(format!("failed to wait for exec child: {error}"), 1))?;

    let stdout = join_child_reader(stdout_thread)?;
    let stderr = join_child_reader(stderr_thread)?;

    if !stdout.is_empty() {
        send_broker_message(broker, BrokerMessage::Stdout(stdout))?;
    }
    if !stderr.is_empty() {
        send_broker_message(broker, BrokerMessage::Stderr(stderr))?;
    }

    Ok(status.success())
}

fn read_child_pipe<R: Read>(mut reader: R, threshold: usize) -> io::Result<Vec<u8>> {
    let mut buffer = SpillBuffer::new(threshold)?;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.write_all(&chunk[..read])?;
    }

    buffer.into_bytes()
}

fn join_child_reader(
    handle: std::thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, Diagnostic> {
    handle
        .join()
        .map_err(|_| Diagnostic::new("internal error: exec output reader thread panicked", 1))?
        .map_err(|error| Diagnostic::new(format!("failed to read exec child output: {error}"), 1))
}

fn send_broker_message(
    broker: &Sender<BrokerMessage>,
    message: BrokerMessage,
) -> Result<(), Diagnostic> {
    broker
        .send(message)
        .map_err(|_| Diagnostic::new("internal error: output broker is unavailable", 1))
}

pub struct SpillBuffer {
    threshold: usize,
    memory: Vec<u8>,
    spill: Option<tempfile::NamedTempFile>,
}

impl SpillBuffer {
    pub fn new(threshold: usize) -> io::Result<Self> {
        Ok(Self {
            threshold,
            memory: Vec::new(),
            spill: None,
        })
    }

    pub fn spilled_path(&self) -> Option<&Path> {
        self.spill.as_ref().map(tempfile::NamedTempFile::path)
    }

    pub fn into_bytes(mut self) -> io::Result<Vec<u8>> {
        if let Some(mut spill) = self.spill.take() {
            let mut bytes = Vec::new();
            spill.rewind()?;
            spill.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }

        Ok(self.memory)
    }
}

impl Write for SpillBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_all(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(spill) = self.spill.as_mut() {
            spill.flush()?;
        }
        Ok(())
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.spill.is_none() && self.memory.len() + bytes.len() <= self.threshold {
            self.memory.extend_from_slice(bytes);
            return Ok(());
        }

        if self.spill.is_none() {
            let mut file = tempfile::NamedTempFile::new()?;
            file.write_all(&self.memory)?;
            self.memory.clear();
            self.spill = Some(file);
        }

        self.spill
            .as_mut()
            .expect("spill exists after initialization")
            .write_all(bytes)
    }
}

pub(crate) fn fixed_batch_cost(spec: &BatchedExecAction) -> usize {
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
    use super::{
        BatchLimit, OrderedActionSink, ParallelActionSink, PendingBatch, SpillBuffer,
        build_batched_argv, compile_batched_exec, delete_path,
    };
    use crate::entry::EntryContext;
    use crate::eval::ActionSink;
    use crate::follow::FollowMode;
    use crate::planner::RuntimeAction;
    use crossbeam_channel::unbounded;
    use std::io::Write;
    use std::os::unix::fs as unix_fs;
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

    #[test]
    fn spill_buffer_moves_large_output_to_a_tempfile() {
        let mut buffer = SpillBuffer::new(8).unwrap();
        buffer.write_all(b"12345678").unwrap();
        buffer.write_all(b"abcdef").unwrap();

        assert!(buffer.spilled_path().is_some());
        assert_eq!(buffer.into_bytes().unwrap(), b"12345678abcdef");
    }

    #[test]
    fn delete_path_unlinks_symlinks_without_touching_targets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("target.txt"), "target\n").unwrap();
        unix_fs::symlink("target.txt", root.path().join("link.txt")).unwrap();

        assert!(delete_path(root.path().join("link.txt").as_path()).unwrap());
        assert!(root.path().join("target.txt").exists());
        assert!(!root.path().join("link.txt").exists());
    }

    #[test]
    fn ordered_flush_reports_batched_false_as_action_failure() {
        let spec = compile_batched_exec(7, &["false".into(), "{}".into()]).unwrap();
        let entry = EntryContext::new(PathBuf::from("placeholder"), 0, true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut sink = OrderedActionSink::new(&mut stdout, &mut stderr);

        let outcome = sink
            .dispatch(
                &RuntimeAction::ExecBatched(spec),
                &entry,
                FollowMode::Physical,
            )
            .unwrap();

        assert!(outcome.matched);
        assert!(!outcome.status.had_action_failures());

        let status = sink.flush().unwrap();
        assert!(status.had_action_failures());
    }

    #[test]
    fn parallel_flush_reports_batched_false_as_action_failure() {
        let spec = compile_batched_exec(9, &["false".into(), "{}".into()]).unwrap();
        let entry = EntryContext::new(PathBuf::from("placeholder"), 0, true);
        let (broker, _rx) = unbounded();
        let mut sink = ParallelActionSink::new(broker, 4).unwrap();

        let outcome = sink
            .dispatch(
                &RuntimeAction::ExecBatched(spec),
                &entry,
                FollowMode::Physical,
            )
            .unwrap();

        assert!(outcome.matched);
        assert!(!outcome.status.had_action_failures());

        let status = sink.flush_all().unwrap();
        assert!(status.had_action_failures());
    }

    #[test]
    fn ordered_delete_missing_path_reports_action_failure_status() {
        let missing = EntryContext::new(PathBuf::from("definitely-missing-delete-target"), 0, true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut sink = OrderedActionSink::new(&mut stdout, &mut stderr);

        let outcome = sink
            .dispatch(&RuntimeAction::Delete, &missing, FollowMode::Physical)
            .unwrap();

        assert!(!outcome.matched);
        assert!(outcome.status.had_action_failures());
    }
}
