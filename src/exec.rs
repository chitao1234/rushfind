use crate::diagnostics::Diagnostic;
use crate::eval::ActionSink;
use crate::output::StdoutSink;
use crate::planner::RuntimeAction;
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

pub struct OrderedActionSink<'a, W: std::io::Write, E: std::io::Write> {
    output: StdoutSink<'a, W>,
    stderr: &'a mut E,
    pending: BTreeMap<ExecBatchId, Vec<PathBuf>>,
}

impl<'a, W: std::io::Write, E: std::io::Write> OrderedActionSink<'a, W, E> {
    pub fn new(stdout: &'a mut W, stderr: &'a mut E) -> Self {
        Self {
            output: StdoutSink::new(stdout),
            stderr,
            pending: BTreeMap::new(),
        }
    }

    fn enqueue(&mut self, spec: &BatchedExecAction, path: &Path) {
        self.pending
            .entry(spec.id)
            .or_default()
            .push(path.to_path_buf());
    }

    pub fn write_diagnostic(&mut self, message: &str) -> Result<(), Diagnostic> {
        writeln!(self.stderr, "{message}")
            .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))
    }
}

impl<W: std::io::Write, E: std::io::Write> ActionSink for OrderedActionSink<'_, W, E> {
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path) -> Result<bool, Diagnostic> {
        match action {
            RuntimeAction::Output(_) => self.output.dispatch(action, path),
            RuntimeAction::ExecImmediate(spec) => run_immediate_ordered(spec, path, self.stderr),
            RuntimeAction::ExecBatched(spec) => {
                self.enqueue(spec, path);
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
