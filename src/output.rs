use crate::diagnostics::Diagnostic;
use crate::eval::ActionSink;
use crate::planner::{OutputAction, RuntimeAction};
use std::io::Write;
use std::path::Path;

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
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path) -> Result<bool, Diagnostic> {
        let RuntimeAction::Output(output) = action else {
            return Err(Diagnostic::new(
                "internal error: plain stdout sink cannot execute runtime actions",
                1,
            ));
        };

        self.writer
            .write_all(&render_output_bytes(*output, path))
            .map_err(|error| Diagnostic::new(format!("failed to write stdout: {error}"), 1))?;
        Ok(true)
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
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path) -> Result<bool, Diagnostic> {
        let RuntimeAction::Output(output) = action else {
            return Err(Diagnostic::new(
                "internal error: recording sink cannot execute runtime actions",
                1,
            ));
        };

        self.bytes
            .extend_from_slice(&render_output_bytes(*output, path));
        Ok(true)
    }
}
