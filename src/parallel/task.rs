use crate::entry::EntryContext;
use crate::runtime_pipeline::SubtreeBarrierId;
use crate::walker::PendingPath;
use std::path::PathBuf;
use std::sync::Arc;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct PreOrderRootTask {
    pub(crate) pending: PendingPath,
}

#[allow(dead_code)]
impl PreOrderRootTask {
    pub(crate) fn for_path(path: PathBuf, depth: usize) -> Self {
        let root_path = Arc::new(path.clone());
        Self {
            pending: PendingPath {
                path,
                root_path,
                depth,
                is_command_line_root: depth == 0,
                physical_file_type_hint: None,
                ancestry: Vec::new(),
                ancestor_barriers: Vec::new(),
                root_device: None,
                parent_completion: None,
            },
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct PostOrderResumeTask {
    pub(crate) entry: EntryContext,
    pub(crate) ancestor_barriers: Vec<SubtreeBarrierId>,
    pub(crate) barrier: SubtreeBarrierId,
    pub(crate) notify_parent: Option<SubtreeBarrierId>,
}

#[allow(dead_code)]
impl PostOrderResumeTask {
    pub(crate) fn for_path(
        path: PathBuf,
        depth: usize,
        barrier: SubtreeBarrierId,
        notify_parent: Option<SubtreeBarrierId>,
    ) -> Self {
        Self {
            entry: EntryContext::new(path, depth, depth == 0),
            ancestor_barriers: Vec::new(),
            barrier,
            notify_parent,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct SiblingChunkTask {
    pub(crate) pending: Vec<PendingPath>,
    pub(crate) completion_barrier: Option<SubtreeBarrierId>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ParallelTask {
    PreOrderRoot(PreOrderRootTask),
    SiblingChunk(SiblingChunkTask),
    PostOrderResume(PostOrderResumeTask),
}
