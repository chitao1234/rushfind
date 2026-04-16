use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::planner::TraversalOptions;
use crate::traversal_control::TraversalControl;
use crossbeam_channel::{Receiver, unbounded};
use std::fs::{self, FileType};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum WalkEvent {
    Entry(EntryContext),
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
}

#[derive(Debug, Clone)]
struct DiscoveredChild {
    path: PathBuf,
    physical_file_type_hint: Option<FileType>,
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
    let mut stack: Vec<PendingPath> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| PendingPath {
            path,
            depth: 0,
            is_command_line_root: true,
            physical_file_type_hint: None,
            ancestry: Vec::new(),
            root_device: None,
        })
        .collect();

    while let Some(pending) = stack.pop() {
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

        emit(WalkEvent::Entry(entry.clone()))?;

        let (child_ancestry, root_device) = match should_descend_directory(
            &pending,
            &entry,
            follow_mode,
            options,
            control,
            backend.as_ref(),
        ) {
            Ok(Some(result)) => result,
            Ok(None) => continue,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        let (children, diagnostics) = match backend.read_children(&pending.path) {
            Ok(result) => result,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        for error in diagnostics {
            emit(WalkEvent::Error(error))?;
        }

        for child in children.into_iter().rev() {
            stack.push(PendingPath {
                path: child.path,
                depth: pending.depth + 1,
                is_command_line_root: false,
                physical_file_type_hint: child.physical_file_type_hint,
                ancestry: child_ancestry.clone(),
                root_device,
            });
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

        thread::spawn(move || {
            loop {
                match work_rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(pending) => {
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
                                    });
                                }
                            }
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                            }
                        }

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
}
