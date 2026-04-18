use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub type FileOutputId = usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFileOutput {
    pub id: FileOutputId,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutputTerminator {
    Newline,
    Nul,
}

pub fn render_file_print_bytes(entry: &EntryContext, terminator: FileOutputTerminator) -> Vec<u8> {
    let rendered = entry.path.to_string_lossy();
    match terminator {
        FileOutputTerminator::Newline => format!("{rendered}\n").into_bytes(),
        FileOutputTerminator::Nul => {
            let mut bytes = rendered.as_bytes().to_vec();
            bytes.push(0);
            bytes
        }
    }
}

pub struct OrderedFileOutputs {
    files: Vec<File>,
}

impl OrderedFileOutputs {
    pub fn open_all(specs: &[PlannedFileOutput]) -> Result<Self, Diagnostic> {
        let mut files = Vec::with_capacity(specs.len());
        for spec in specs {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&spec.path)
                .map_err(|error| Diagnostic::new(format!("{}: {error}", spec.path.display()), 1))?;
            files.push(file);
        }
        Ok(Self { files })
    }

    pub fn write_record(&mut self, id: FileOutputId, bytes: &[u8]) -> Result<(), Diagnostic> {
        self.files[id]
            .write_all(bytes)
            .map_err(|error| Diagnostic::new(format!("failed to write file output: {error}"), 1))
    }
}
