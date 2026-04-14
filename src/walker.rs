use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::follow::FollowMode;
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

pub fn walk_ordered<F>(
    start_paths: &[PathBuf],
    follow_mode: FollowMode,
    options: TraversalOptions,
    mut emit: F,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<(), Diagnostic>,
{
    let mut stack: Vec<(PathBuf, usize, bool)> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| (path, 0, true))
        .collect();

    while let Some((path, depth, is_command_line_root)) = stack.pop() {
        let entry = match load_entry(&path, depth, is_command_line_root) {
            Ok(entry) => entry,
            Err(error) => {
                emit(WalkEvent::Error(error))?;
                continue;
            }
        };

        emit(WalkEvent::Entry(entry.clone()))?;

        if should_descend_with_follow_mode(&entry, follow_mode, options.max_depth) {
            let read_dir = match fs::read_dir(&path) {
                Ok(read_dir) => read_dir,
                Err(error) => {
                    emit(WalkEvent::Error(path_error(&path, error)))?;
                    continue;
                }
            };

            let mut children = Vec::new();
            for child in read_dir {
                match child {
                    Ok(child) => children.push(child.path()),
                    Err(error) => emit(WalkEvent::Error(path_error(&path, error)))?,
                }
            }

            for child in children.into_iter().rev() {
                stack.push((child, depth + 1, false));
            }
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
    let (work_tx, work_rx) = unbounded::<(PathBuf, usize, bool)>();
    let (event_tx, event_rx) = unbounded::<WalkEvent>();
    let inflight = Arc::new(AtomicUsize::new(0));

    for path in start_paths {
        inflight.fetch_add(1, Ordering::SeqCst);
        work_tx.send((path.clone(), 0, true)).unwrap();
    }

    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let work_tx = work_tx.clone();
        let event_tx = event_tx.clone();
        let inflight = inflight.clone();

        thread::spawn(move || {
            loop {
                match work_rx.recv_timeout(Duration::from_millis(25)) {
                    Ok((path, depth, is_command_line_root)) => {
                        let entry = match load_entry(&path, depth, is_command_line_root) {
                            Ok(entry) => entry,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(error));
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        let _ = event_tx.send(WalkEvent::Entry(entry.clone()));

                        if should_descend_with_follow_mode(&entry, follow_mode, options.max_depth) {
                            match fs::read_dir(&path) {
                                Ok(read_dir) => {
                                    for child in read_dir {
                                        match child {
                                            Ok(child) => {
                                                inflight.fetch_add(1, Ordering::SeqCst);
                                                let _ =
                                                    work_tx.send((child.path(), depth + 1, false));
                                            }
                                            Err(error) => {
                                                let _ = event_tx.send(WalkEvent::Error(
                                                    path_error(&path, error),
                                                ));
                                            }
                                        }
                                    }
                                }
                                Err(error) => {
                                    let _ =
                                        event_tx.send(WalkEvent::Error(path_error(&path, error)));
                                }
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

fn should_descend_with_follow_mode(
    entry: &EntryContext,
    follow_mode: FollowMode,
    max_depth: Option<usize>,
) -> bool {
    should_descend(entry.depth, max_depth) && entry.active_kind(follow_mode) == EntryKind::Directory
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
