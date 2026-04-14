use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::planner::TraversalOptions;
use crossbeam_channel::{Receiver, unbounded};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkEvent {
    Entry(EntryContext),
    Error(Diagnostic),
}

pub fn walk_ordered<F>(
    start_paths: &[PathBuf],
    options: TraversalOptions,
    mut emit: F,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<(), Diagnostic>,
{
    let mut stack: Vec<(PathBuf, usize)> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| (path, 0))
        .collect();

    while let Some((path, depth)) = stack.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                emit(WalkEvent::Error(Diagnostic::new(
                    format!("{}: {error}", path.display()),
                    1,
                )))?;
                continue;
            }
        };

        let kind = file_type_to_kind(&metadata.file_type());
        emit(WalkEvent::Entry(EntryContext::synthetic(
            path.clone(),
            kind,
            depth,
        )))?;

        if kind == EntryKind::Directory && should_descend(depth, options.max_depth) {
            let read_dir = match fs::read_dir(&path) {
                Ok(read_dir) => read_dir,
                Err(error) => {
                    emit(WalkEvent::Error(Diagnostic::new(
                        format!("{}: {error}", path.display()),
                        1,
                    )))?;
                    continue;
                }
            };

            let mut children = Vec::new();
            for child in read_dir {
                match child {
                    Ok(child) => children.push(child.path()),
                    Err(error) => emit(WalkEvent::Error(Diagnostic::new(
                        format!("{}: {error}", path.display()),
                        1,
                    )))?,
                }
            }

            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }

    Ok(())
}

pub fn walk_parallel(
    start_paths: &[PathBuf],
    options: TraversalOptions,
    workers: usize,
) -> Receiver<WalkEvent> {
    let (work_tx, work_rx) = unbounded::<(PathBuf, usize)>();
    let (event_tx, event_rx) = unbounded::<WalkEvent>();
    let inflight = Arc::new(AtomicUsize::new(0));

    for path in start_paths {
        inflight.fetch_add(1, Ordering::SeqCst);
        work_tx.send((path.clone(), 0)).unwrap();
    }

    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let work_tx = work_tx.clone();
        let event_tx = event_tx.clone();
        let inflight = inflight.clone();

        thread::spawn(move || {
            loop {
                match work_rx.recv_timeout(Duration::from_millis(25)) {
                    Ok((path, depth)) => {
                        let metadata = match fs::symlink_metadata(&path) {
                            Ok(metadata) => metadata,
                            Err(error) => {
                                let _ = event_tx.send(WalkEvent::Error(Diagnostic::new(
                                    format!("{}: {error}", path.display()),
                                    1,
                                )));
                                inflight.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                        };

                        let kind = file_type_to_kind(&metadata.file_type());
                        let _ = event_tx.send(WalkEvent::Entry(EntryContext::synthetic(
                            path.clone(),
                            kind,
                            depth,
                        )));

                        if kind == EntryKind::Directory && should_descend(depth, options.max_depth)
                        {
                            match fs::read_dir(&path) {
                                Ok(read_dir) => {
                                    for child in read_dir {
                                        match child {
                                            Ok(child) => {
                                                inflight.fetch_add(1, Ordering::SeqCst);
                                                let _ = work_tx.send((child.path(), depth + 1));
                                            }
                                            Err(error) => {
                                                let _ = event_tx.send(WalkEvent::Error(
                                                    Diagnostic::new(
                                                        format!("{}: {error}", path.display()),
                                                        1,
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                }
                                Err(error) => {
                                    let _ = event_tx.send(WalkEvent::Error(Diagnostic::new(
                                        format!("{}: {error}", path.display()),
                                        1,
                                    )));
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

fn should_descend(depth: usize, max_depth: Option<usize>) -> bool {
    match max_depth {
        Some(max) => depth < max,
        None => true,
    }
}

pub fn file_type_to_kind(file_type: &fs::FileType) -> EntryKind {
    if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Unknown
    }
}
