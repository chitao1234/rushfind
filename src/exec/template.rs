use crate::diagnostics::Diagnostic;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub type ExecBatchId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecSemantics {
    Normal,
    DirLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecTemplateSegment {
    Literal(OsString),
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExecCommand {
    pub cwd: Option<PathBuf>,
    pub argv: Vec<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmediateExecAction {
    pub semantics: ExecSemantics,
    pub argv: Vec<Vec<ExecTemplateSegment>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchedExecAction {
    pub id: ExecBatchId,
    pub semantics: ExecSemantics,
    pub argv_prefix: Vec<OsString>,
}

impl ImmediateExecAction {
    pub fn command_cwd(&self, path: &Path) -> Option<PathBuf> {
        match self.semantics {
            ExecSemantics::Normal => None,
            ExecSemantics::DirLocal => Some(execdir_cwd(path)),
        }
    }
}

impl BatchedExecAction {
    pub fn batch_cwd(&self, path: &Path) -> Option<PathBuf> {
        match self.semantics {
            ExecSemantics::Normal => None,
            ExecSemantics::DirLocal => Some(execdir_cwd(path)),
        }
    }

    pub fn batch_flag(&self) -> &'static str {
        match self.semantics {
            ExecSemantics::Normal => "-exec ... +",
            ExecSemantics::DirLocal => "-execdir ... +",
        }
    }
}

pub fn compile_immediate_exec(semantics: ExecSemantics, argv: &[OsString]) -> ImmediateExecAction {
    ImmediateExecAction {
        semantics,
        argv: argv
            .iter()
            .map(|arg| compile_segments(arg.as_os_str()))
            .collect(),
    }
}

pub fn compile_batched_exec(
    id: ExecBatchId,
    semantics: ExecSemantics,
    argv: &[OsString],
) -> Result<BatchedExecAction, Diagnostic> {
    let batch_placeholder_error = format!(
        "`{}` requires exactly one standalone `{{}}` as the final command argument",
        match semantics {
            ExecSemantics::Normal => "-exec ... +",
            ExecSemantics::DirLocal => "-execdir ... +",
        }
    );

    let placeholder_indexes = argv
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| is_standalone_placeholder(arg.as_os_str()).then_some(index))
        .collect::<Vec<_>>();

    let Some(&placeholder_index) = placeholder_indexes.first() else {
        return Err(Diagnostic::parse(batch_placeholder_error.clone()));
    };

    if placeholder_indexes.len() != 1 || placeholder_index + 1 != argv.len() {
        return Err(Diagnostic::parse(batch_placeholder_error.clone()));
    }

    if argv.iter().any(|arg| {
        !is_standalone_placeholder(arg.as_os_str()) && contains_placeholder(arg.as_os_str())
    }) {
        return Err(Diagnostic::parse(batch_placeholder_error));
    }

    Ok(BatchedExecAction {
        id,
        semantics,
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

pub fn execdir_cwd(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn rendered_path(path: &Path, semantics: ExecSemantics) -> OsString {
    match semantics {
        ExecSemantics::Normal => path.as_os_str().to_os_string(),
        ExecSemantics::DirLocal => {
            let basename = path.file_name().expect("matched path has a basename");
            let mut bytes = b"./".to_vec();
            bytes.extend_from_slice(basename.as_bytes());
            OsString::from_vec(bytes)
        }
    }
}

pub fn render_immediate_argv(spec: &ImmediateExecAction, path: &Path) -> Vec<OsString> {
    let rendered_path = rendered_path(path, spec.semantics);
    let path_bytes = rendered_path.as_os_str().as_bytes();

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

pub fn build_immediate_command(spec: &ImmediateExecAction, path: &Path) -> PreparedExecCommand {
    PreparedExecCommand {
        cwd: spec.command_cwd(path),
        argv: render_immediate_argv(spec, path),
    }
}

pub fn batched_path_cost(spec: &BatchedExecAction, path: &Path) -> usize {
    rendered_path(path, spec.semantics)
        .as_os_str()
        .as_bytes()
        .len()
        + 1
}

pub fn build_batched_argv(
    spec: &BatchedExecAction,
    paths: &[PathBuf],
) -> Result<PreparedExecCommand, Diagnostic> {
    let cwd = match spec.semantics {
        ExecSemantics::Normal => None,
        ExecSemantics::DirLocal => Some(execdir_cwd(paths.first().ok_or_else(|| {
            Diagnostic::new("internal error: batched exec action missing paths", 1)
        })?)),
    };

    let mut argv = spec.argv_prefix.clone();
    argv.extend(paths.iter().map(|path| rendered_path(path, spec.semantics)));
    Ok(PreparedExecCommand { cwd, argv })
}
