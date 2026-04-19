use crate::diagnostics::{Diagnostic, failed_to_write, internal_poisoned};
use crate::entry::EntryContext;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    crate::action_output::render_file_print_bytes(entry, terminator)
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
            .map_err(|error| failed_to_write("file output", error))
    }
}

#[derive(Clone)]
pub struct SharedFileOutputs {
    files: Arc<Vec<Mutex<File>>>,
}

impl SharedFileOutputs {
    pub fn open_all(specs: &[PlannedFileOutput]) -> Result<Self, Diagnostic> {
        let mut files = Vec::with_capacity(specs.len());
        for spec in specs {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&spec.path)
                .map_err(|error| Diagnostic::new(format!("{}: {error}", spec.path.display()), 1))?;
            files.push(Mutex::new(file));
        }
        Ok(Self {
            files: Arc::new(files),
        })
    }

    pub fn write_record(&self, id: FileOutputId, bytes: &[u8]) -> Result<(), Diagnostic> {
        let mut file = self.files[id]
            .lock()
            .map_err(|_| internal_poisoned("file output lock"))?;
        file.write_all(bytes)
            .map_err(|error| failed_to_write("file output", error))
    }
}
