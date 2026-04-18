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
