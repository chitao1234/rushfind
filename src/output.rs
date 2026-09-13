use crate::action_output::{RenderedAction, render_action_output};
use crate::diagnostics::{Diagnostic, failed_to_write};
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, ActionSink, EvalContext};
use crate::follow::FollowMode;
use crate::planner::{OutputAction, RuntimeAction};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::{BufWriter, IsTerminal, Stdout, Write};
use std::thread::{Scope, ScopedJoinHandle};

/// C stdio block-buffers a stream that is not a terminal and line-buffers one
/// that is, and GNU find inherits that. Rust's `Stdout` is a `LineWriter`
/// either way, so a piped or redirected `-print` pays one `write` per record:
/// measured on 345k records, 59% of the run.
enum StdoutBuffer {
    Line(Stdout),
    Block(BufWriter<Stdout>),
}

pub struct BufferedStdout {
    inner: StdoutBuffer,
}

impl BufferedStdout {
    pub fn new() -> Self {
        let stdout = std::io::stdout();
        let inner = if stdout.is_terminal() {
            StdoutBuffer::Line(stdout)
        } else {
            StdoutBuffer::Block(BufWriter::with_capacity(64 * 1024, stdout))
        };
        Self { inner }
    }
}

impl Default for BufferedStdout {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for BufferedStdout {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.inner {
            StdoutBuffer::Line(writer) => writer.write(buf),
            StdoutBuffer::Block(writer) => writer.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.inner {
            StdoutBuffer::Line(writer) => writer.flush(),
            StdoutBuffer::Block(writer) => writer.flush(),
        }
    }
}

#[derive(Debug)]
pub enum BrokerMessage {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

pub fn spawn_broker<'scope, W, E>(
    scope: &'scope Scope<'scope, '_>,
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
                .map_err(|error| failed_to_write("stdout", error))?,
            // Block buffering must not let a diagnostic overtake the records
            // that were written before it.
            BrokerMessage::Stderr(bytes) => {
                stdout
                    .flush()
                    .map_err(|error| failed_to_write("stdout", error))?;
                stderr
                    .write_all(&bytes)
                    .map_err(|error| failed_to_write("stderr", error))?;
            }
        }
    }

    stdout
        .flush()
        .map_err(|error| failed_to_write("stdout", error))?;

    Ok(())
}

pub fn render_output_bytes(action: OutputAction, entry: &EntryContext) -> Vec<u8> {
    crate::action_output::render_output_bytes(action, entry)
}

pub struct StdoutSink<'a, W: Write> {
    writer: &'a mut W,
}

impl<'a, W: Write> StdoutSink<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), Diagnostic> {
        self.writer
            .write_all(bytes)
            .map_err(|error| failed_to_write("stdout", error))
    }

    pub(crate) fn flush(&mut self) -> Result<(), Diagnostic> {
        self.writer
            .flush()
            .map_err(|error| failed_to_write("stdout", error))
    }
}

impl<W: Write> ActionSink for StdoutSink<'_, W> {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        match render_action_output(action, entry, follow_mode, context)? {
            Some(RenderedAction::Stdout(bytes)) => {
                self.write_bytes(&bytes)?;
                Ok(ActionOutcome::matched_true())
            }
            Some(RenderedAction::File { .. }) | None => Err(Diagnostic::new(
                "internal error: plain stdout sink cannot execute runtime actions",
                1,
            )),
        }
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

    fn record_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
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
        match render_action_output(action, entry, follow_mode, context)? {
            Some(RenderedAction::Stdout(bytes)) => {
                self.record_bytes(&bytes);
                Ok(ActionOutcome::matched_true())
            }
            Some(RenderedAction::File { .. }) | None => Err(Diagnostic::new(
                "internal error: recording sink cannot execute runtime actions",
                1,
            )),
        }
    }
}
