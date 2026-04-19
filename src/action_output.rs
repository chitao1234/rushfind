use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::EvalContext;
use crate::file_output::{FileOutputId, FileOutputTerminator};
use crate::follow::FollowMode;
use crate::planner::{OutputAction, RuntimeAction};
use std::os::unix::ffi::OsStrExt;

pub(crate) enum RenderedAction {
    Stdout(Vec<u8>),
    File { destination: FileOutputId, bytes: Vec<u8> },
}

fn render_output_bytes(action: OutputAction, entry: &EntryContext) -> Vec<u8> {
    let mut bytes = entry.path.as_os_str().as_bytes().to_vec();
    match action {
        OutputAction::Print => bytes.push(b'\n'),
        OutputAction::Print0 => bytes.push(0),
    }
    bytes
}

fn render_file_print_bytes(entry: &EntryContext, terminator: FileOutputTerminator) -> Vec<u8> {
    let mut bytes = entry.path.as_os_str().as_bytes().to_vec();
    match terminator {
        FileOutputTerminator::Newline => bytes.push(b'\n'),
        FileOutputTerminator::Nul => bytes.push(0),
    }
    bytes
}

pub(crate) fn render_action_output(
    action: &RuntimeAction,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Option<RenderedAction>, Diagnostic> {
    match action {
        RuntimeAction::Output(output) => Ok(Some(RenderedAction::Stdout(render_output_bytes(
            *output, entry,
        )))),
        RuntimeAction::Printf(program) => Ok(Some(RenderedAction::Stdout(
            crate::printf::render_printf_bytes(program, entry, follow_mode, context)?,
        ))),
        RuntimeAction::Ls => Ok(Some(RenderedAction::Stdout(
            crate::ls::render_ls_record(entry, follow_mode, context)?,
        ))),
        RuntimeAction::FilePrint {
            destination,
            terminator,
        } => Ok(Some(RenderedAction::File {
            destination: *destination,
            bytes: render_file_print_bytes(entry, *terminator),
        })),
        RuntimeAction::FilePrintf {
            destination,
            program,
        } => Ok(Some(RenderedAction::File {
            destination: *destination,
            bytes: crate::printf::render_printf_bytes(program, entry, follow_mode, context)?,
        })),
        RuntimeAction::FileLs { destination } => Ok(Some(RenderedAction::File {
            destination: *destination,
            bytes: crate::ls::render_ls_record(entry, follow_mode, context)?,
        })),
        RuntimeAction::Quit
        | RuntimeAction::ExecImmediate(_)
        | RuntimeAction::ExecBatched(_)
        | RuntimeAction::Delete => Ok(None),
    }
}
