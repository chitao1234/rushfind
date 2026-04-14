use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::planner::TraversalOptions;
use std::fs;
use std::path::PathBuf;

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
