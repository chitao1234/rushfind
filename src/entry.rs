use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Block,
    Character,
    Fifo,
    Socket,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryContext {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub depth: usize,
}

impl EntryContext {
    pub fn synthetic(path: PathBuf, kind: EntryKind, depth: usize) -> Self {
        Self { path, kind, depth }
    }
}
