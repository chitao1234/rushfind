use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, ActionSink, EvalContext};
use crate::follow::FollowMode;
use crate::planner::{OutputAction, RuntimeAction};
use crate::printf::render_printf_bytes;
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::thread::{Scope, ScopedJoinHandle};

#[derive(Debug)]
pub enum BrokerMessage {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

pub fn spawn_broker<'scope, 'env, W, E>(
    scope: &'scope Scope<'scope, 'env>,
    stdout: &'scope mut W,
    stderr: &'scope mut E,
) -> (
    Sender<BrokerMessage>,
    ScopedJoinHandle<'scope, Result<(), Diagnostic>>,
)
where
    W: Write + Send,
    E: Write + Send,
{
    let (tx, rx): (Sender<BrokerMessage>, Receiver<BrokerMessage>) = unbounded();
    let handle = scope.spawn(move || broker_loop(rx, stdout, stderr));
    (tx, handle)
}

fn broker_loop<W: Write, E: Write>(
    rx: Receiver<BrokerMessage>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), Diagnostic> {
    while let Ok(message) = rx.recv() {
        match message {
            BrokerMessage::Stdout(bytes) => stdout
                .write_all(&bytes)
                .map_err(|error| Diagnostic::new(format!("failed to write stdout: {error}"), 1))?,
            BrokerMessage::Stderr(bytes) => stderr
                .write_all(&bytes)
                .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))?,
        }
    }

    Ok(())
}

pub fn render_output_bytes(action: OutputAction, entry: &EntryContext) -> Vec<u8> {
    let mut bytes = entry.path.as_os_str().as_bytes().to_vec();
    match action {
        OutputAction::Print => {
            bytes.push(b'\n');
            bytes
        }
        OutputAction::Print0 => {
            bytes.push(0);
            bytes
        }
    }
}

pub(crate) fn render_runtime_action_bytes(
    action: &RuntimeAction,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    match action {
        RuntimeAction::Output(output) => Ok(render_output_bytes(*output, entry)),
        RuntimeAction::Printf(program) => render_printf_bytes(program, entry, follow_mode, context),
        RuntimeAction::Ls => crate::ls::render_ls_record(entry, follow_mode, context),
        _ => Err(Diagnostic::new(
            "internal error: runtime action does not render to stdout bytes",
            1,
        )),
    }
}

pub struct StdoutSink<'a, W: Write> {
    writer: &'a mut W,
}

impl<'a, W: Write> StdoutSink<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }
}

impl<'a, W: Write> ActionSink for StdoutSink<'a, W> {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        let (RuntimeAction::Output(_) | RuntimeAction::Printf(_) | RuntimeAction::Ls) = action else {
            return Err(Diagnostic::new(
                "internal error: plain stdout sink cannot execute runtime actions",
                1,
            ));
        };

        self.writer
            .write_all(&render_runtime_action_bytes(
                action,
                entry,
                follow_mode,
                context,
            )?)
            .map_err(|error| Diagnostic::new(format!("failed to write stdout: {error}"), 1))?;
        Ok(ActionOutcome::matched_true())
    }
}

#[derive(Debug, Default)]
pub struct RecordingSink {
    bytes: Vec<u8>,
}

impl RecordingSink {
    pub fn into_utf8(self) -> String {
        String::from_utf8(self.bytes).expect("recording sink must contain utf-8")
    }
}

impl ActionSink for RecordingSink {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        let (RuntimeAction::Output(_) | RuntimeAction::Printf(_) | RuntimeAction::Ls) = action else {
            return Err(Diagnostic::new(
                "internal error: recording sink cannot execute runtime actions",
                1,
            ));
        };

        self.bytes.extend_from_slice(&render_runtime_action_bytes(
            action,
            entry,
            follow_mode,
            context,
        )?);
        Ok(ActionOutcome::matched_true())
    }
}
