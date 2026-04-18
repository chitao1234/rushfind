use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::parallel::task::PostOrderResumeTask;
use crate::planner::{TraversalOptions, TraversalOrder};
use crate::runtime_pipeline::SubtreeBarrierId;
use crate::traversal_control::TraversalControl;
use crate::walker::{
    FsWalkBackend, PendingPath, WalkBackend, WalkEvent, scheduled_entry, should_descend_directory,
};
use crossbeam_channel::{Receiver, unbounded};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type CompletionId = usize;

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

#[derive(Debug)]
struct CompletionRecord {
    entry: EntryContext,
    ancestor_barriers: Vec<SubtreeBarrierId>,
    parent: Option<CompletionId>,
    remaining_children: usize,
    traversal_complete: bool,
}

#[derive(Debug, Default)]
struct CompletionState {
    next_id: AtomicUsize,
    records: Mutex<BTreeMap<CompletionId, CompletionRecord>>,
}

#[derive(Debug)]
struct ReleasedDirectory {
    id: CompletionId,
    entry: EntryContext,
    ancestor_barriers: Vec<SubtreeBarrierId>,
}

pub(crate) fn walk_parallel<C>(
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    workers: usize,
    control: C,
) -> Receiver<WalkEvent>
where
    C: Fn(&EntryContext) -> Result<TraversalControl, Diagnostic> + Send + Sync + 'static,
{
    walk_parallel_with_backend(
        Arc::new(FsWalkBackend),
        start_paths,
        follow_mode,
        options,
        workers,
        control,
    )
}

pub(crate) fn walk_parallel_with_backend<C>(
    backend: Arc<dyn WalkBackend>,
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    workers: usize,
    control: C,
) -> Receiver<WalkEvent>
where
    C: Fn(&EntryContext) -> Result<TraversalControl, Diagnostic> + Send + Sync + 'static,
{
    let (work_tx, work_rx) = unbounded::<PendingPath>();
    let (event_tx, event_rx) = unbounded::<WalkEvent>();
    let inflight = Arc::new(AtomicUsize::new(0));
    let next_sequence = Arc::new(AtomicU64::new(0));
    let control = Arc::new(control);
    let completions = Arc::new(CompletionState::default());

    for path in start_paths {
        inflight.fetch_add(1, Ordering::SeqCst);
        work_tx
            .send(PendingPath {
                path: path.clone(),
                root_path: Arc::new(path.clone()),
                depth: 0,
                is_command_line_root: true,
                physical_file_type_hint: None,
                ancestry: Vec::new(),
                ancestor_barriers: Vec::new(),
                root_device: None,
                parent_completion: None,
            })
            .unwrap();
    }

    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let work_tx = work_tx.clone();
        let event_tx = event_tx.clone();
        let inflight = inflight.clone();
        let next_sequence = next_sequence.clone();
        let backend = backend.clone();
        let control = control.clone();
        let completions = completions.clone();

        thread::spawn(move || {
            loop {
                match work_rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(pending) => {
                        if options.order == TraversalOrder::PreOrder {
                            let entry = match backend.load_entry(&pending) {
                                Ok(entry) => entry,
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                            };

                            let control = match control(&entry) {
                                Ok(control) => control,
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                            };

                            let _ = event_tx.send(WalkEvent::Entry(scheduled_entry(
                                entry.clone(),
                                next_sequence.fetch_add(1, Ordering::SeqCst),
                                pending.ancestor_barriers.clone(),
                                None,
                            )));

                            let (child_ancestry, root_device) = match should_descend_directory(
                                &pending,
                                &entry,
                                follow_mode,
                                options,
                                control,
                                backend.as_ref(),
                            ) {
                                Ok(Some(result)) => result,
                                Ok(None) => {
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                            };

                            match backend.read_children(&pending.path) {
                                Ok((children, diagnostics)) => {
                                    for error in diagnostics {
                                        let _ = event_tx.send(WalkEvent::Error(error));
                                    }

                                    for child in children {
                                        inflight.fetch_add(1, Ordering::SeqCst);
                                        let _ = work_tx.send(PendingPath {
                                            path: child.path,
                                            root_path: pending.root_path.clone(),
                                            depth: pending.depth + 1,
                                            is_command_line_root: false,
                                            physical_file_type_hint: child.physical_file_type_hint,
                                            ancestry: child_ancestry.clone(),
                                            ancestor_barriers: pending.ancestor_barriers.clone(),
                                            root_device,
                                            parent_completion: None,
                                        });
                                    }
                                }
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                }
                            }

                            inflight.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }

                        let entry = match backend.load_entry(&pending) {
                            Ok(entry) => entry,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                let _ = child_finished(
                                    completions.as_ref(),
                                    pending.parent_completion,
                                    &event_tx,
                                    next_sequence.as_ref(),
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        let control = match control(&entry) {
                            Ok(control) => control,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                let _ = child_finished(
                                    completions.as_ref(),
                                    pending.parent_completion,
                                    &event_tx,
                                    next_sequence.as_ref(),
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        let is_directory =
                            match backend.active_directory_identity(&entry, follow_mode) {
                                Ok(identity) => identity.is_some(),
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                    let _ = child_finished(
                                        completions.as_ref(),
                                        pending.parent_completion,
                                        &event_tx,
                                        next_sequence.as_ref(),
                                    );
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                            };

                        if !is_directory {
                            let _ = event_tx.send(WalkEvent::Entry(scheduled_entry(
                                entry.clone(),
                                next_sequence.fetch_add(1, Ordering::SeqCst),
                                pending.ancestor_barriers.clone(),
                                None,
                            )));
                            let _ = child_finished(
                                completions.as_ref(),
                                pending.parent_completion,
                                &event_tx,
                                next_sequence.as_ref(),
                            );
                            inflight.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }

                        let completion_id = match begin_directory(
                            completions.as_ref(),
                            entry.clone(),
                            pending.ancestor_barriers.clone(),
                            pending.parent_completion,
                        ) {
                            Ok(id) => id,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                let _ = child_finished(
                                    completions.as_ref(),
                                    pending.parent_completion,
                                    &event_tx,
                                    next_sequence.as_ref(),
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };
                        let barrier = SubtreeBarrierId(completion_id);

                        let (child_ancestry, root_device) = match should_descend_directory(
                            &pending,
                            &entry,
                            follow_mode,
                            options,
                            control,
                            backend.as_ref(),
                        ) {
                            Ok(Some(result)) => result,
                            Ok(None) => {
                                let _ = mark_directory_ready(
                                    completions.as_ref(),
                                    completion_id,
                                    &event_tx,
                                    next_sequence.as_ref(),
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                let _ = mark_directory_ready(
                                    completions.as_ref(),
                                    completion_id,
                                    &event_tx,
                                    next_sequence.as_ref(),
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        match backend.read_children(&pending.path) {
                            Ok((children, diagnostics)) => {
                                for error in diagnostics {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                }

                                let child_ancestor_barriers = vec![barrier];
                                for child in children {
                                    let _ =
                                        increment_child_count(completions.as_ref(), completion_id);
                                    inflight.fetch_add(1, Ordering::SeqCst);
                                    let _ = work_tx.send(PendingPath {
                                        path: child.path,
                                        root_path: pending.root_path.clone(),
                                        depth: pending.depth + 1,
                                        is_command_line_root: false,
                                        physical_file_type_hint: child.physical_file_type_hint,
                                        ancestry: child_ancestry.clone(),
                                        ancestor_barriers: child_ancestor_barriers.clone(),
                                        root_device,
                                        parent_completion: Some(completion_id),
                                    });
                                }
                            }
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                            }
                        }

                        let _ = mark_directory_ready(
                            completions.as_ref(),
                            completion_id,
                            &event_tx,
                            next_sequence.as_ref(),
                        );
                        inflight.fetch_sub(1, Ordering::SeqCst);
                    }
                    Err(_) if inflight.load(Ordering::SeqCst) == 0 => break,
                    Err(_) => continue,
                }
            }
        });
    }

    drop(work_tx);
    drop(event_tx);
    event_rx
}

fn begin_directory(
    state: &CompletionState,
    entry: EntryContext,
    ancestor_barriers: Vec<SubtreeBarrierId>,
    parent: Option<CompletionId>,
) -> Result<CompletionId, Diagnostic> {
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    state
        .records
        .lock()
        .map_err(|_| Diagnostic::new("internal error: completion state poisoned", 1))?
        .insert(
            id,
            CompletionRecord {
                entry,
                ancestor_barriers,
                parent,
                remaining_children: 0,
                traversal_complete: false,
            },
        );
    Ok(id)
}

fn increment_child_count(state: &CompletionState, id: CompletionId) -> Result<(), Diagnostic> {
    let mut records = state
        .records
        .lock()
        .map_err(|_| Diagnostic::new("internal error: completion state poisoned", 1))?;
    let record = records
        .get_mut(&id)
        .ok_or_else(|| Diagnostic::new("internal error: missing completion record", 1))?;
    record.remaining_children += 1;
    Ok(())
}

fn mark_directory_ready(
    state: &CompletionState,
    id: CompletionId,
    event_tx: &crossbeam_channel::Sender<WalkEvent>,
    next_sequence: &AtomicU64,
) -> Result<(), Diagnostic> {
    let released = {
        let mut records = state
            .records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: completion state poisoned", 1))?;
        let record = records
            .get_mut(&id)
            .ok_or_else(|| Diagnostic::new("internal error: missing completion record", 1))?;
        record.traversal_complete = true;
        collect_released_entries(&mut records, Some(id), false)
    };

    emit_directory_completions(event_tx, released, next_sequence)
}

fn child_finished(
    state: &CompletionState,
    parent: Option<CompletionId>,
    event_tx: &crossbeam_channel::Sender<WalkEvent>,
    next_sequence: &AtomicU64,
) -> Result<(), Diagnostic> {
    let Some(parent) = parent else {
        return Ok(());
    };

    let released = {
        let mut records = state
            .records
            .lock()
            .map_err(|_| Diagnostic::new("internal error: completion state poisoned", 1))?;
        collect_released_entries(&mut records, Some(parent), true)
    };

    emit_directory_completions(event_tx, released, next_sequence)
}

fn collect_released_entries(
    records: &mut BTreeMap<CompletionId, CompletionRecord>,
    start: Option<CompletionId>,
    mut decrement_first: bool,
) -> Vec<ReleasedDirectory> {
    let mut released = Vec::new();
    let mut current = start;

    while let Some(id) = current {
        let Some(record) = records.get_mut(&id) else {
            break;
        };

        if decrement_first {
            record.remaining_children = record.remaining_children.saturating_sub(1);
        }

        if !record.traversal_complete || record.remaining_children != 0 {
            break;
        }

        let CompletionRecord {
            entry,
            ancestor_barriers,
            parent,
            ..
        } = records.remove(&id).expect("completion record exists");
        released.push(ReleasedDirectory {
            id,
            entry,
            ancestor_barriers,
        });
        current = parent;
        decrement_first = true;
    }

    released
}

fn emit_directory_completions(
    event_tx: &crossbeam_channel::Sender<WalkEvent>,
    released: Vec<ReleasedDirectory>,
    next_sequence: &AtomicU64,
) -> Result<(), Diagnostic> {
    for item in released {
        event_tx
            .send(WalkEvent::DirectoryComplete(scheduled_entry(
                item.entry,
                next_sequence.fetch_add(1, Ordering::SeqCst),
                item.ancestor_barriers,
                Some(SubtreeBarrierId(item.id)),
            )))
            .map_err(|_| Diagnostic::new("internal error: walk event channel unavailable", 1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resume_task_is_released_only_after_the_last_spilled_child_finishes() {
        let table = BarrierTable::default();
        let entry = EntryContext::new(PathBuf::from("dir"), 1, false);
        let barrier = table.begin_directory(entry.clone(), Vec::new(), 2, None).unwrap();

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
