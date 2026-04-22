use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::parallel::task::PostOrderResumeTask;
use crate::runtime_pipeline::SubtreeBarrierId;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
enum BarrierRecord {
    Directory {
        entry: EntryContext,
        remaining_spilled_chunks: usize,
        notify_parent: Option<SubtreeBarrierId>,
    },
    Batch {
        remaining_roots: usize,
        notify_parent: SubtreeBarrierId,
    },
}

#[derive(Debug)]
pub(crate) enum BarrierRelease {
    Resume(PostOrderResumeTask),
    NotifyParent(SubtreeBarrierId),
}

#[derive(Debug, Default)]
pub(crate) struct BarrierTable {
    next_id: AtomicUsize,
    records: Mutex<BTreeMap<SubtreeBarrierId, BarrierRecord>>,
}

impl BarrierTable {
    pub(crate) fn begin_directory(
        &self,
        entry: EntryContext,
        spilled_children: usize,
        notify_parent: Option<SubtreeBarrierId>,
    ) -> Result<SubtreeBarrierId, Diagnostic> {
        let barrier = SubtreeBarrierId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: barrier table poisoned", 1))?
            .insert(
                barrier,
                BarrierRecord::Directory {
                    entry,
                    remaining_spilled_chunks: spilled_children,
                    notify_parent,
                },
            );
        Ok(barrier)
    }

    pub(crate) fn begin_batch(
        &self,
        roots: usize,
        notify_parent: SubtreeBarrierId,
    ) -> Result<SubtreeBarrierId, Diagnostic> {
        let barrier = SubtreeBarrierId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: barrier table poisoned", 1))?
            .insert(
                barrier,
                BarrierRecord::Batch {
                    remaining_roots: roots,
                    notify_parent,
                },
            );
        Ok(barrier)
    }

    pub(crate) fn finish_spilled_chunk(
        &self,
        barrier: SubtreeBarrierId,
    ) -> Result<Option<BarrierRelease>, Diagnostic> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: barrier table poisoned", 1))?;
        let Some(record) = records.get_mut(&barrier) else {
            return Ok(None);
        };

        match record {
            BarrierRecord::Directory {
                remaining_spilled_chunks,
                ..
            } => {
                if *remaining_spilled_chunks > 1 {
                    *remaining_spilled_chunks -= 1;
                    return Ok(None);
                }
            }
            BarrierRecord::Batch {
                remaining_roots, ..
            } => {
                if *remaining_roots > 1 {
                    *remaining_roots -= 1;
                    return Ok(None);
                }
            }
        }

        let record = records.remove(&barrier).expect("barrier record exists");
        Ok(Some(match record {
            BarrierRecord::Directory {
                entry,
                notify_parent,
                ..
            } => BarrierRelease::Resume(PostOrderResumeTask {
                entry,
                notify_parent,
            }),
            BarrierRecord::Batch { notify_parent, .. } => {
                BarrierRelease::NotifyParent(notify_parent)
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resume_task_is_released_only_after_the_last_spilled_chunk_finishes() {
        let table = BarrierTable::default();
        let entry = EntryContext::new(PathBuf::from("dir"), 1, false);
        let barrier = table.begin_directory(entry.clone(), 2, None).unwrap();

        assert!(table.finish_spilled_chunk(barrier).unwrap().is_none());

        let resume = table.finish_spilled_chunk(barrier).unwrap().unwrap();
        match resume {
            BarrierRelease::Resume(resume) => {
                assert_eq!(resume.entry.path, entry.path);
            }
            BarrierRelease::NotifyParent(_) => {
                panic!("directory barrier should release a resume task");
            }
        }
    }

    #[test]
    fn released_resume_task_preserves_parent_notification() {
        let table = BarrierTable::default();
        let parent = SubtreeBarrierId(3);
        let barrier = table
            .begin_directory(
                EntryContext::new(PathBuf::from("dir"), 1, false),
                1,
                Some(parent),
            )
            .unwrap();

        let resume = table.finish_spilled_chunk(barrier).unwrap().unwrap();
        match resume {
            BarrierRelease::Resume(resume) => {
                assert_eq!(resume.notify_parent, Some(parent));
            }
            BarrierRelease::NotifyParent(_) => {
                panic!("directory barrier should release a resume task");
            }
        }
    }
}
