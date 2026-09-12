use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::EvalContext;
use crate::file_output::{FileOutputId, FileOutputTerminator};
use crate::follow::FollowMode;
use crate::planner::{OutputAction, RuntimeAction};

pub(crate) enum RenderedAction {
    Stdout(Vec<u8>),
    File {
        destination: FileOutputId,
        bytes: Vec<u8>,
    },
}

pub(crate) struct OutputPresentation<'a> {
    pub(crate) ctype_profile: &'a crate::ctype::CtypeProfile,
    pub(crate) stdout_is_tty: bool,
}

impl<'a> OutputPresentation<'a> {
    pub(crate) fn raw(ctype_profile: &'a crate::ctype::CtypeProfile) -> Self {
        Self {
            ctype_profile,
            stdout_is_tty: false,
        }
    }
}

pub(crate) fn render_output_bytes(action: OutputAction, entry: &EntryContext) -> Vec<u8> {
    let mut bytes = crate::platform::path::display_bytes(&entry.path);
    match action {
        OutputAction::Print => bytes.push(b'\n'),
        OutputAction::Print0 => bytes.push(0),
    }
    bytes
}

pub(crate) fn render_file_print_bytes(
    entry: &EntryContext,
    terminator: FileOutputTerminator,
) -> Vec<u8> {
    let mut bytes = crate::platform::path::display_bytes(&entry.path);
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
        RuntimeAction::Ls => Ok(Some(RenderedAction::Stdout(crate::ls::render_ls_record(
            entry,
            follow_mode,
            context,
        )?))),
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
        | RuntimeAction::ExecPrompt(_)
        | RuntimeAction::Delete => Ok(None),
    }
}

pub(crate) fn render_action_output_with_presentation(
    action: &RuntimeAction,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    presentation: &OutputPresentation<'_>,
) -> Result<Option<RenderedAction>, Diagnostic> {
    let _ = (presentation.ctype_profile, presentation.stdout_is_tty);
    render_action_output(action, entry, follow_mode, context)
}
