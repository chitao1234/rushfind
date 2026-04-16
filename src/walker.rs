use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::planner::{TraversalOptions, TraversalOrder};
use crate::traversal_control::TraversalControl;
use crossbeam_channel::{Receiver, unbounded};
use std::collections::BTreeMap;
use std::fs::{self, FileType};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum WalkEvent {
    Entry(EntryContext),
    DirectoryComplete(EntryContext),
    Error(Diagnostic),
}

#[derive(Debug, Clone)]
struct PendingPath {
    path: PathBuf,
    depth: usize,
    is_command_line_root: bool,
    physical_file_type_hint: Option<FileType>,
    ancestry: Vec<FileIdentity>,
    root_device: Option<u64>,
    parent_completion: Option<CompletionId>,
}

#[derive(Debug, Clone)]
enum OrderedFrame {
    Visit(PendingPath),
    Complete(EntryContext),
}

#[derive(Debug, Clone)]
struct DiscoveredChild {
    path: PathBuf,
    physical_file_type_hint: Option<FileType>,
}

type CompletionId = usize;

#[derive(Debug)]
struct CompletionRecord {
    entry: EntryContext,
    parent: Option<CompletionId>,
    remaining_children: usize,
    traversal_complete: bool,
}

#[derive(Debug, Default)]
struct CompletionState {
    next_id: AtomicUsize,
    records: Mutex<BTreeMap<CompletionId, CompletionRecord>>,
}

trait WalkBackend: Send + Sync + 'static {
    fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic>;
    fn read_children(
        &self,
        path: &Path,
    ) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic>;
    fn active_directory_identity(
        &self,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<Option<FileIdentity>, Diagnostic>;
}

struct FsWalkBackend;

impl WalkBackend for FsWalkBackend {
    fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
        load_entry(pending)
    }

    fn read_children(
        &self,
        path: &Path,
    ) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic> {
        read_children(path)
    }

    fn active_directory_identity(
        &self,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<Option<FileIdentity>, Diagnostic> {
        entry.active_directory_identity(follow_mode)
    }
}

pub(crate) fn walk_ordered<F, C>(
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    control: C,
    emit: F,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<(), Diagnostic>,
    C: Fn(&EntryContext) -> Result<TraversalControl, Diagnostic>,
{
    walk_ordered_with_backend(
        Arc::new(FsWalkBackend),
        start_paths,
        follow_mode,
        options,
        control,
        emit,
    )
}

fn walk_ordered_with_backend<F, C>(
    backend: Arc<dyn WalkBackend>,
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    control: C,
    mut emit: F,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<(), Diagnostic>,
    C: Fn(&EntryContext) -> Result<TraversalControl, Diagnostic>,
{
    let mut stack: Vec<OrderedFrame> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| {
            OrderedFrame::Visit(PendingPath {
                path,
                depth: 0,
                is_command_line_root: true,
                physical_file_type_hint: None,
                ancestry: Vec::new(),
                root_device: None,
                parent_completion: None,
            })
        })
        .collect();

    while let Some(frame) = stack.pop() {
        let pending = match frame {
            OrderedFrame::Visit(pending) => pending,
            OrderedFrame::Complete(entry) => {
                emit(WalkEvent::DirectoryComplete(entry))?;
                continue;
            }
        };

        let entry = match backend.load_entry(&pending) {
            Ok(entry) => entry,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        let control = match control(&entry) {
            Ok(control) => control,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        let is_directory = match backend.active_directory_identity(&entry, follow_mode) {
            Ok(identity) => identity.is_some(),
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        match options.order {
            TraversalOrder::PreOrder => emit(WalkEvent::Entry(entry.clone()))?,
            TraversalOrder::DepthFirstPostOrder if !is_directory => {
                emit(WalkEvent::Entry(entry.clone()))?
            }
            TraversalOrder::DepthFirstPostOrder => {}
        }

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
                if options.order == TraversalOrder::DepthFirstPostOrder && is_directory {
                    emit(WalkEvent::DirectoryComplete(entry.clone()))?;
                }
                continue;
            }
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        let (children, diagnostics) = match backend.read_children(&pending.path) {
            Ok(result) => result,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                if options.order == TraversalOrder::DepthFirstPostOrder && is_directory {
                    emit(WalkEvent::DirectoryComplete(entry.clone()))?;
                }
                continue;
            }
        };

        for error in diagnostics {
            emit(WalkEvent::Error(error))?;
        }

        if options.order == TraversalOrder::DepthFirstPostOrder && is_directory {
            stack.push(OrderedFrame::Complete(entry.clone()));
        }

        for child in children.into_iter().rev() {
            stack.push(OrderedFrame::Visit(PendingPath {
                path: child.path,
                depth: pending.depth + 1,
                is_command_line_root: false,
                physical_file_type_hint: child.physical_file_type_hint,
                ancestry: child_ancestry.clone(),
                root_device,
                parent_completion: None,
            }));
        }
    }

    Ok(())
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

fn walk_parallel_with_backend<C>(
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
    let control = Arc::new(control);
    let completions = Arc::new(CompletionState::default());

    for path in start_paths {
        inflight.fetch_add(1, Ordering::SeqCst);
        work_tx
            .send(PendingPath {
                path: path.clone(),
                depth: 0,
                is_command_line_root: true,
                physical_file_type_hint: None,
                ancestry: Vec::new(),
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

                            let _ = event_tx.send(WalkEvent::Entry(entry.clone()));

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
                                            depth: pending.depth + 1,
                                            is_command_line_root: false,
                                            physical_file_type_hint: child.physical_file_type_hint,
                                            ancestry: child_ancestry.clone(),
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
                                    );
                                    inflight.fetch_sub(1, Ordering::SeqCst);
                                    continue;
                                }
                            };

                        if !is_directory {
                            let _ = event_tx.send(WalkEvent::Entry(entry.clone()));
                            let _ = child_finished(
                                completions.as_ref(),
                                pending.parent_completion,
                                &event_tx,
                            );
                            inflight.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }

                        let completion_id = match begin_directory(
                            completions.as_ref(),
                            entry.clone(),
                            pending.parent_completion,
                        ) {
                            Ok(id) => id,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                let _ = child_finished(
                                    completions.as_ref(),
                                    pending.parent_completion,
                                    &event_tx,
                                );
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

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

                                for child in children {
                                    let _ =
                                        increment_child_count(completions.as_ref(), completion_id);
                                    inflight.fetch_add(1, Ordering::SeqCst);
                                    let _ = work_tx.send(PendingPath {
                                        path: child.path,
                                        depth: pending.depth + 1,
                                        is_command_line_root: false,
                                        physical_file_type_hint: child.physical_file_type_hint,
                                        ancestry: child_ancestry.clone(),
                                        root_device,
                                        parent_completion: Some(completion_id),
                                    });
                                }
                            }
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                            }
                        }

                        let _ =
                            mark_directory_ready(completions.as_ref(), completion_id, &event_tx);
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

    emit_directory_completions(event_tx, released)
}

fn child_finished(
    state: &CompletionState,
    parent: Option<CompletionId>,
    event_tx: &crossbeam_channel::Sender<WalkEvent>,
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

    emit_directory_completions(event_tx, released)
}

fn collect_released_entries(
    records: &mut BTreeMap<CompletionId, CompletionRecord>,
    start: Option<CompletionId>,
    mut decrement_first: bool,
) -> Vec<EntryContext> {
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

        let CompletionRecord { entry, parent, .. } =
            records.remove(&id).expect("completion record exists");
        released.push(entry);
        current = parent;
        decrement_first = true;
    }

    released
}

fn emit_directory_completions(
    event_tx: &crossbeam_channel::Sender<WalkEvent>,
    released: Vec<EntryContext>,
) -> Result<(), Diagnostic> {
    for entry in released {
        event_tx
            .send(WalkEvent::DirectoryComplete(entry))
            .map_err(|_| Diagnostic::new("internal error: walk event channel unavailable", 1))?;
    }
    Ok(())
}

fn load_entry(pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
    let entry = EntryContext::with_file_type_hint(
        pending.path.clone(),
        pending.depth,
        pending.is_command_line_root,
        pending.physical_file_type_hint,
    );

    if pending.is_command_line_root {
        entry.physical_kind()?;
    }

    Ok(entry)
}

fn should_descend_directory(
    pending: &PendingPath,
    entry: &EntryContext,
    follow_mode: FollowMode,
    options: TraversalOptions,
    control: TraversalControl,
    backend: &dyn WalkBackend,
) -> Result<Option<(Vec<FileIdentity>, Option<u64>)>, Diagnostic> {
    if control.prune || !should_descend(pending.depth, options.max_depth) {
        return Ok(None);
    }

    let Some(directory_identity) = backend.active_directory_identity(entry, follow_mode)? else {
        return Ok(None);
    };

    if pending.ancestry.contains(&directory_identity) {
        return Err(loop_error(&pending.path));
    }

    let root_device = pending.root_device.or(Some(directory_identity.dev));
    if options.same_file_system
        && root_device.is_some_and(|device| directory_identity.dev != device)
    {
        return Ok(None);
    }

    let mut next = pending.ancestry.clone();
    next.push(directory_identity);
    Ok(Some((next, root_device)))
}

fn read_children(path: &Path) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic> {
    let read_dir = fs::read_dir(path).map_err(|error| path_error(path, error))?;
    let mut children = Vec::new();
    let mut diagnostics = Vec::new();

    for child in read_dir {
        match child {
            Ok(child) => children.push(DiscoveredChild {
                path: child.path(),
                physical_file_type_hint: child.file_type().ok(),
            }),
            Err(error) => diagnostics.push(path_error(path, error)),
        }
    }

    Ok((children, diagnostics))
}

fn should_descend(depth: usize, max_depth: Option<usize>) -> bool {
    match max_depth {
        Some(max) => depth < max,
        None => true,
    }
}

fn path_error(path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(format!("{}: {error}", path.display()), 1)
}

fn loop_error(path: &Path) -> Diagnostic {
    Diagnostic::new(format!("filesystem loop detected at {}", path.display()), 1)
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveredChild, PendingPath, WalkBackend, WalkEvent, read_children,
        walk_ordered_with_backend, walk_parallel_with_backend,
    };
    use crate::diagnostics::Diagnostic;
    use crate::entry::EntryContext;
    use crate::follow::FollowMode;
    use crate::identity::FileIdentity;
    use crate::planner::{TraversalOptions, TraversalOrder};
    use crate::traversal_control::TraversalControl;
    use crossbeam_channel::Receiver;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn ordered_walk_respects_prune_boundary_before_child_fanout() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("keep")).unwrap();
        fs::create_dir(root.path().join("skip")).unwrap();
        fs::write(root.path().join("keep/file.txt"), "keep\n").unwrap();
        fs::write(root.path().join("skip/file.txt"), "skip\n").unwrap();

        let backend = Arc::new(TestBackend::default());
        let mut seen = Vec::new();
        walk_ordered_with_backend(
            backend,
            &[root.path().to_path_buf()],
            FollowMode::Physical,
            TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::PreOrder,
            },
            |entry| {
                let prune = entry.path.file_name().is_some_and(|name| name == "skip");
                Ok(TraversalControl {
                    matched: true,
                    prune,
                })
            },
            |event| {
                if let WalkEvent::Entry(entry) = event {
                    seen.push(entry.path);
                }
                Ok(())
            },
        )
        .unwrap();

        assert!(seen.iter().any(|path| path.ends_with("skip")));
        assert!(!seen.iter().any(|path| path.ends_with("skip/file.txt")));
        assert!(seen.iter().any(|path| path.ends_with("keep/file.txt")));
    }

    #[test]
    fn parallel_walk_applies_same_filesystem_per_root() {
        let root = tempdir().unwrap();
        let left = root.path().join("left");
        let right = root.path().join("right");
        fs::create_dir(&left).unwrap();
        fs::create_dir(&right).unwrap();
        fs::create_dir(left.join("local")).unwrap();
        fs::create_dir(left.join("remote")).unwrap();
        fs::create_dir(right.join("local")).unwrap();
        fs::create_dir(right.join("remote")).unwrap();
        fs::write(left.join("local/keep.txt"), "left-local\n").unwrap();
        fs::write(left.join("remote/skip.txt"), "left-remote\n").unwrap();
        fs::write(right.join("local/keep.txt"), "right-local\n").unwrap();
        fs::write(right.join("remote/skip.txt"), "right-remote\n").unwrap();

        let backend = Arc::new(TestBackend::with_devices(BTreeMap::from([
            (left.clone(), 1),
            (left.join("local"), 1),
            (left.join("remote"), 2),
            (right.clone(), 2),
            (right.join("local"), 2),
            (right.join("remote"), 1),
        ])));

        let receiver = walk_parallel_with_backend(
            backend,
            &[left.clone(), right.clone()],
            FollowMode::Physical,
            TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: true,
                order: TraversalOrder::PreOrder,
            },
            4,
            |_entry| {
                Ok(TraversalControl {
                    matched: true,
                    prune: false,
                })
            },
        );

        let seen = collect_paths(receiver);
        assert!(seen.iter().any(|path| path.ends_with("left/remote")));
        assert!(seen.iter().any(|path| path.ends_with("right/remote")));
        assert!(
            !seen
                .iter()
                .any(|path| path.ends_with("left/remote/skip.txt"))
        );
        assert!(
            !seen
                .iter()
                .any(|path| path.ends_with("right/remote/skip.txt"))
        );
        assert!(
            seen.iter()
                .any(|path| path.ends_with("left/local/keep.txt"))
        );
        assert!(
            seen.iter()
                .any(|path| path.ends_with("right/local/keep.txt"))
        );
    }

    #[test]
    fn ordered_depth_mode_emits_directory_completion_after_descendants() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(root.path().join("dir/file.txt"), "child\n").unwrap();

        let mut seen = Vec::new();
        walk_ordered_with_backend(
            Arc::new(TestBackend::default()),
            &[root.path().to_path_buf()],
            FollowMode::Physical,
            TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::DepthFirstPostOrder,
            },
            |_entry| {
                Ok(TraversalControl {
                    matched: true,
                    prune: false,
                })
            },
            |event| {
                match event {
                    WalkEvent::Entry(entry) => {
                        let rel = entry.path.strip_prefix(root.path()).unwrap();
                        seen.push(format!("entry:{}", rel.display()));
                    }
                    WalkEvent::DirectoryComplete(entry) => {
                        let rel = entry.path.strip_prefix(root.path()).unwrap();
                        seen.push(format!("done:{}", rel.display()));
                    }
                    WalkEvent::Error(error) => panic!("unexpected walk error: {error:?}"),
                }
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            seen,
            vec![
                "entry:dir/file.txt".to_string(),
                "done:dir".to_string(),
                "done:".to_string(),
            ]
        );
    }

    #[test]
    fn parallel_depth_mode_completes_parent_after_descendants() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(root.path().join("dir/file.txt"), "child\n").unwrap();

        let seen = collect_paths_and_completions(
            root.path(),
            walk_parallel_with_backend(
                Arc::new(TestBackend::default()),
                &[root.path().to_path_buf()],
                FollowMode::Physical,
                TraversalOptions {
                    min_depth: 0,
                    max_depth: None,
                    same_file_system: false,
                    order: TraversalOrder::DepthFirstPostOrder,
                },
                4,
                |_entry| {
                    Ok(TraversalControl {
                        matched: true,
                        prune: false,
                    })
                },
            ),
        );

        let file_index = seen
            .iter()
            .position(|label| label == "entry:dir/file.txt")
            .unwrap();
        let dir_index = seen.iter().position(|label| label == "done:dir").unwrap();
        assert!(file_index < dir_index);
    }

    #[derive(Default)]
    struct TestBackend {
        devices: BTreeMap<PathBuf, u64>,
    }

    impl TestBackend {
        fn with_devices(devices: BTreeMap<PathBuf, u64>) -> Self {
            Self { devices }
        }
    }

    impl WalkBackend for TestBackend {
        fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
            Ok(EntryContext::with_file_type_hint(
                pending.path.clone(),
                pending.depth,
                pending.is_command_line_root,
                pending.physical_file_type_hint,
            ))
        }

        fn read_children(
            &self,
            path: &Path,
        ) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic> {
            read_children(path)
        }

        fn active_directory_identity(
            &self,
            entry: &EntryContext,
            follow_mode: FollowMode,
        ) -> Result<Option<FileIdentity>, Diagnostic> {
            let identity = entry.active_directory_identity(follow_mode)?;
            Ok(identity.map(|identity| FileIdentity {
                dev: self
                    .devices
                    .get(&entry.path)
                    .copied()
                    .unwrap_or(identity.dev),
                ino: identity.ino,
            }))
        }
    }

    fn collect_paths(receiver: Receiver<WalkEvent>) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for event in receiver {
            if let WalkEvent::Entry(entry) = event {
                paths.push(entry.path);
            }
        }
        paths
    }

    fn collect_paths_and_completions(base: &Path, receiver: Receiver<WalkEvent>) -> Vec<String> {
        let mut seen = Vec::new();
        for event in receiver {
            match event {
                WalkEvent::Entry(entry) => {
                    let rel = entry.path.strip_prefix(base).unwrap().display();
                    seen.push(format!("entry:{rel}"));
                }
                WalkEvent::DirectoryComplete(entry) => {
                    let rel = entry.path.strip_prefix(base).unwrap().display();
                    seen.push(format!("done:{rel}"));
                }
                WalkEvent::Error(error) => panic!("unexpected walk error: {error:?}"),
            }
        }
        seen
    }
}
