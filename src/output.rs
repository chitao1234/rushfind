use crate::diagnostics::Diagnostic;
use crate::planner::OutputAction;
use std::io::Write;
use std::path::Path;

pub trait OutputSink {
    fn emit(&mut self, action: OutputAction, path: &Path) -> Result<(), Diagnostic>;
}

pub struct StdoutSink<'a, W: Write> {
    writer: &'a mut W,
}

impl<'a, W: Write> StdoutSink<'a, W> {
    pub fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }
}

impl<'a, W: Write> OutputSink for StdoutSink<'a, W> {
    fn emit(&mut self, action: OutputAction, path: &Path) -> Result<(), Diagnostic> {
        let rendered = path.to_string_lossy();
        let bytes = match action {
            OutputAction::Print => format!("{rendered}\n").into_bytes(),
            OutputAction::Print0 => {
                let mut bytes = rendered.as_bytes().to_vec();
                bytes.push(0);
                bytes
            }
        };

        self.writer
            .write_all(&bytes)
            .map_err(|error| Diagnostic::new(format!("failed to write stdout: {error}"), 1))
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

impl OutputSink for RecordingSink {
    fn emit(&mut self, action: OutputAction, path: &Path) -> Result<(), Diagnostic> {
        let rendered = path.to_string_lossy();
        match action {
            OutputAction::Print => self
                .bytes
                .extend_from_slice(format!("{rendered}\n").as_bytes()),
            OutputAction::Print0 => {
                self.bytes.extend_from_slice(rendered.as_bytes());
                self.bytes.push(0);
            }
        }
        Ok(())
    }
}
