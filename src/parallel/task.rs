use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct SubtreeTask {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
}

#[allow(dead_code)]
impl SubtreeTask {
    pub(crate) fn new(path: PathBuf, depth: usize) -> Self {
        Self { path, depth }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ParallelTask {
    PreOrder(SubtreeTask),
}
