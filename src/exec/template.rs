use crate::diagnostics::Diagnostic;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

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
