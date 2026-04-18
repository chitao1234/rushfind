use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::parallel::task::PostOrderResumeTask;
use crate::runtime_pipeline::SubtreeBarrierId;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct BarrierRecord {
    entry: EntryContext,
    ancestor_barriers: Vec<SubtreeBarrierId>,
    remaining_spilled_children: usize,
    notify_parent: Option<SubtreeBarrierId>,
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
        ancestor_barriers: Vec<SubtreeBarrierId>,
        spilled_children: usize,
        notify_parent: Option<SubtreeBarrierId>,
    ) -> Result<SubtreeBarrierId, Diagnostic> {
        let barrier = SubtreeBarrierId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: barrier table poisoned", 1))?
            .insert(
                barrier,
                BarrierRecord {
                    entry,
                    ancestor_barriers,
                    remaining_spilled_children: spilled_children,
                    notify_parent,
                },
            );
        Ok(barrier)
    }

    pub(crate) fn finish_spilled_child(
        &self,
        barrier: SubtreeBarrierId,
    ) -> Result<Option<PostOrderResumeTask>, Diagnostic> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: barrier table poisoned", 1))?;
        let Some(record) = records.get_mut(&barrier) else {
            return Ok(None);
        };

        if record.remaining_spilled_children > 1 {
            record.remaining_spilled_children -= 1;
            return Ok(None);
        }

        let record = records.remove(&barrier).expect("barrier record exists");
        Ok(Some(PostOrderResumeTask {
            entry: record.entry,
            ancestor_barriers: record.ancestor_barriers,
            barrier,
            notify_parent: record.notify_parent,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resume_task_is_released_only_after_the_last_spilled_child_finishes() {
        let table = BarrierTable::default();
        let entry = EntryContext::new(PathBuf::from("dir"), 1, false);
        let barrier = table
            .begin_directory(entry.clone(), Vec::new(), 2, None)
            .unwrap();

        assert!(table.finish_spilled_child(barrier).unwrap().is_none());

        let resume = table.finish_spilled_child(barrier).unwrap().unwrap();
        assert_eq!(resume.entry.path, entry.path);
        assert_eq!(resume.barrier, barrier);
    }

    #[test]
    fn released_resume_task_preserves_parent_notification() {
        let table = BarrierTable::default();
        let parent = SubtreeBarrierId(3);
        let barrier = table
            .begin_directory(
                EntryContext::new(PathBuf::from("dir"), 1, false),
                vec![parent],
                1,
                Some(parent),
            )
            .unwrap();

        let resume = table.finish_spilled_child(barrier).unwrap().unwrap();
        assert_eq!(resume.notify_parent, Some(parent));
        assert_eq!(resume.ancestor_barriers, vec![parent]);
    }
}
