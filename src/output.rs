use crate::diagnostics::Diagnostic;
use crate::eval::{ActionOutcome, ActionSink};
use crate::planner::{OutputAction, RuntimeAction};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::Write;
use std::path::Path;
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

pub fn render_output_bytes(action: OutputAction, path: &Path) -> Vec<u8> {
    let rendered = path.to_string_lossy();
    match action {
        OutputAction::Print => format!("{rendered}\n").into_bytes(),
        OutputAction::Print0 => {
            let mut bytes = rendered.as_bytes().to_vec();
            bytes.push(0);
            bytes
        }
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
        path: &Path,
    ) -> Result<ActionOutcome, Diagnostic> {
        let RuntimeAction::Output(output) = action else {
            return Err(Diagnostic::new(
                "internal error: plain stdout sink cannot execute runtime actions",
                1,
            ));
        };

        self.writer
            .write_all(&render_output_bytes(*output, path))
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
        path: &Path,
    ) -> Result<ActionOutcome, Diagnostic> {
        let RuntimeAction::Output(output) = action else {
            return Err(Diagnostic::new(
                "internal error: recording sink cannot execute runtime actions",
                1,
            ));
        };

        self.bytes
            .extend_from_slice(&render_output_bytes(*output, path));
        Ok(ActionOutcome::matched_true())
    }
}
