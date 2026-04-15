use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::planner::TraversalOptions;
use crossbeam_channel::{Receiver, unbounded};
use std::fs;
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
    ancestry: Vec<FileIdentity>,
}

pub fn walk_ordered<F>(
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    mut emit: F,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<(), Diagnostic>,
{
    let mut stack: Vec<PendingPath> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| PendingPath {
            path,
            depth: 0,
            is_command_line_root: true,
            ancestry: Vec::new(),
        })
        .collect();

    while let Some(pending) = stack.pop() {
        let entry = match load_entry(&pending.path, pending.depth, pending.is_command_line_root) {
            Ok(entry) => entry,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        emit(WalkEvent::Entry(entry.clone()))?;

        let child_ancestry =
            match child_ancestry_for(&pending, &entry, follow_mode, options.max_depth) {
                Ok(Some(ancestry)) => ancestry,
                Ok(None) => continue,
                Err(error) => {
                    emit(WalkEvent::Error(error))?;
                    continue;
                }
            };

        let (children, diagnostics) = match read_children(&pending.path) {
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
                path: child,
                depth: pending.depth + 1,
                is_command_line_root: false,
                ancestry: child_ancestry.clone(),
            });
        }
    }

    Ok(())
}

pub fn walk_parallel(
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    workers: usize,
) -> Receiver<WalkEvent> {
    let (work_tx, work_rx) = unbounded::<PendingPath>();
    let (event_tx, event_rx) = unbounded::<WalkEvent>();
    let inflight = Arc::new(AtomicUsize::new(0));

    for path in start_paths {
        inflight.fetch_add(1, Ordering::SeqCst);
        work_tx
            .send(PendingPath {
                path: path.clone(),
                depth: 0,
                is_command_line_root: true,
                ancestry: Vec::new(),
            })
            .unwrap();
    }

    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let work_tx = work_tx.clone();
        let event_tx = event_tx.clone();
        let inflight = inflight.clone();

        thread::spawn(move || {
            loop {
                match work_rx.recv_timeout(Duration::from_millis(25)) {
                    Ok(pending) => {
                        let entry = match load_entry(
                            &pending.path,
                            pending.depth,
                            pending.is_command_line_root,
                        ) {
                            Ok(entry) => entry,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        let _ = event_tx.send(WalkEvent::Entry(entry.clone()));

                        let child_ancestry = match child_ancestry_for(
                            &pending,
                            &entry,
                            follow_mode,
                            options.max_depth,
                        ) {
                            Ok(Some(ancestry)) => ancestry,
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

                        match read_children(&pending.path) {
                            Ok((children, diagnostics)) => {
                                for error in diagnostics {
                                    let _ = event_tx.send(WalkEvent::Error(error));
                                }

                                for child in children {
                                    inflight.fetch_add(1, Ordering::SeqCst);
                                    let _ = work_tx.send(PendingPath {
                                        path: child,
                                        depth: pending.depth + 1,
                                        is_command_line_root: false,
                                        ancestry: child_ancestry.clone(),
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

fn load_entry(
    path: &Path,
    depth: usize,
    is_command_line_root: bool,
) -> Result<EntryContext, Diagnostic> {
    let physical_metadata = fs::symlink_metadata(path).map_err(|error| path_error(path, error))?;
    let logical_metadata = fs::metadata(path).ok();

    Ok(EntryContext::new(
        path.to_path_buf(),
        depth,
        is_command_line_root,
        physical_metadata,
        logical_metadata,
    ))
}

fn child_ancestry_for(
    pending: &PendingPath,
    entry: &EntryContext,
    follow_mode: FollowMode,
    max_depth: Option<usize>,
) -> Result<Option<Vec<FileIdentity>>, Diagnostic> {
    if !should_descend(pending.depth, max_depth) {
        return Ok(None);
    }

    let Some(directory_identity) = entry.active_directory_identity(follow_mode) else {
        return Ok(None);
    };

    if pending.ancestry.contains(&directory_identity) {
        return Err(loop_error(&pending.path));
    }

    let mut next = pending.ancestry.clone();
    next.push(directory_identity);
    Ok(Some(next))
}

fn read_children(path: &Path) -> Result<(Vec<PathBuf>, Vec<Diagnostic>), Diagnostic> {
    let read_dir = fs::read_dir(path).map_err(|error| path_error(path, error))?;
    let mut children = Vec::new();
    let mut diagnostics = Vec::new();

    for child in read_dir {
        match child {
            Ok(child) => children.push(child.path()),
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
