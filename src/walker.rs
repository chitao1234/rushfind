use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::planner::{TraversalOptions, TraversalOrder};
use crate::runtime_pipeline::{EntryTicket, SubtreeBarrierId};
use crate::traversal_control::TraversalControl;
use std::fs::{self, FileType};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ScheduledEntry {
    pub entry: EntryContext,
    #[allow(dead_code)]
    pub(crate) ticket: EntryTicket,
}

impl std::ops::Deref for ScheduledEntry {
    type Target = EntryContext;

    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

#[derive(Debug, Clone)]
pub enum WalkEvent {
    Entry(ScheduledEntry),
    DirectoryComplete(ScheduledEntry),
    Error(Diagnostic),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderedWalkDirective {
    Continue,
    Stop,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPath {
    pub(crate) path: PathBuf,
    pub(crate) root_path: Arc<PathBuf>,
    pub(crate) depth: usize,
    pub(crate) is_command_line_root: bool,
    pub(crate) physical_file_type_hint: Option<FileType>,
    pub(crate) ancestry: Vec<FileIdentity>,
    pub(crate) ancestor_barriers: Vec<SubtreeBarrierId>,
    pub(crate) root_device: Option<u64>,
    pub(crate) parent_completion: Option<usize>,
}

#[derive(Debug, Clone)]
enum OrderedFrame {
    Visit(PendingPath),
    Complete {
        entry: EntryContext,
        ancestor_barriers: Vec<SubtreeBarrierId>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredChild {
    pub(crate) path: PathBuf,
    pub(crate) physical_file_type_hint: Option<FileType>,
}

pub(crate) fn scheduled_entry(
    entry: EntryContext,
    sequence: u64,
    ancestor_barriers: Vec<SubtreeBarrierId>,
    block_on_subtree: Option<SubtreeBarrierId>,
) -> ScheduledEntry {
    ScheduledEntry {
        entry,
        ticket: EntryTicket {
            sequence,
            ancestor_barriers,
            block_on_subtree,
        },
    }
}

pub(crate) trait WalkBackend: Send + Sync + 'static {
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

pub(crate) struct FsWalkBackend;

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
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
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
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
    C: Fn(&EntryContext) -> Result<TraversalControl, Diagnostic>,
{
    let mut stack: Vec<OrderedFrame> = start_paths
        .iter()
        .rev()
        .cloned()
        .map(|path| {
            let root_path = Arc::new(path.clone());
            OrderedFrame::Visit(PendingPath {
                path,
                root_path,
                depth: 0,
                is_command_line_root: true,
                physical_file_type_hint: None,
                ancestry: Vec::new(),
                ancestor_barriers: Vec::new(),
                root_device: None,
                parent_completion: None,
            })
        })
        .collect();
    let mut next_sequence = 0_u64;

    while let Some(frame) = stack.pop() {
        let pending = match frame {
            OrderedFrame::Visit(pending) => pending,
            OrderedFrame::Complete {
                entry,
                ancestor_barriers,
            } => {
                if emit(WalkEvent::DirectoryComplete(scheduled_entry(
                    entry,
                    next_sequence,
                    ancestor_barriers,
                    None,
                )))? == OrderedWalkDirective::Stop
                {
                    return Ok(());
                }
                next_sequence += 1;
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
            TraversalOrder::PreOrder => {
                if emit(WalkEvent::Entry(scheduled_entry(
                    entry.clone(),
                    next_sequence,
                    pending.ancestor_barriers.clone(),
                    None,
                )))? == OrderedWalkDirective::Stop
                {
                    return Ok(());
                }
                next_sequence += 1;
            }
            TraversalOrder::DepthFirstPostOrder if !is_directory => {
                if emit(WalkEvent::Entry(scheduled_entry(
                    entry.clone(),
                    next_sequence,
                    pending.ancestor_barriers.clone(),
                    None,
                )))? == OrderedWalkDirective::Stop
                {
                    return Ok(());
                }
                next_sequence += 1;
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
                    if emit(WalkEvent::DirectoryComplete(scheduled_entry(
                        entry.clone(),
                        next_sequence,
                        pending.ancestor_barriers.clone(),
                        None,
                    )))? == OrderedWalkDirective::Stop
                    {
                        return Ok(());
                    }
                    next_sequence += 1;
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
                    if emit(WalkEvent::DirectoryComplete(scheduled_entry(
                        entry.clone(),
                        next_sequence,
                        pending.ancestor_barriers.clone(),
                        None,
                    )))? == OrderedWalkDirective::Stop
                    {
                        return Ok(());
                    }
                    next_sequence += 1;
                }
                continue;
            }
        };

        for error in diagnostics {
            emit(WalkEvent::Error(error))?;
        }

        if options.order == TraversalOrder::DepthFirstPostOrder && is_directory {
            stack.push(OrderedFrame::Complete {
                entry: entry.clone(),
                ancestor_barriers: pending.ancestor_barriers.clone(),
            });
        }

        for child in children.into_iter().rev() {
            stack.push(OrderedFrame::Visit(PendingPath {
                path: child.path,
                root_path: pending.root_path.clone(),
                depth: pending.depth + 1,
                is_command_line_root: false,
                physical_file_type_hint: child.physical_file_type_hint,
                ancestry: child_ancestry.clone(),
                ancestor_barriers: pending.ancestor_barriers.clone(),
                root_device,
                parent_completion: None,
            }));
        }
    }

    Ok(())
}

pub(crate) fn load_entry(pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
    let entry = EntryContext::with_file_type_hint_and_root(
        pending.path.clone(),
        pending.depth,
        pending.is_command_line_root,
        pending.root_path.clone(),
        pending.physical_file_type_hint,
    );

    if pending.is_command_line_root {
        entry.physical_kind()?;
    }

    Ok(entry)
}

pub(crate) fn should_descend_directory(
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

pub(crate) fn read_children(
    path: &Path,
) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic> {
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
        DiscoveredChild, OrderedWalkDirective, PendingPath, WalkBackend, WalkEvent, read_children,
        walk_ordered_with_backend,
    };
    use crate::diagnostics::Diagnostic;
    use crate::entry::EntryContext;
    use crate::follow::FollowMode;
    use crate::identity::FileIdentity;
    use crate::planner::{TraversalOptions, TraversalOrder};
    use crate::traversal_control::TraversalControl;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn ordered_walk_respects_prune_boundary_before_child_fanout() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("keep")).unwrap();
        fs::create_dir(root.path().join("skip")).unwrap();
        fs::write(root.path().join("keep/file.txt"), "keep\n").unwrap();
        fs::write(root.path().join("skip/file.txt"), "skip\n").unwrap();

        let backend = Arc::new(TestBackend);
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
                if let WalkEvent::Entry(item) = event {
                    seen.push(item.entry.path);
                }
                Ok(OrderedWalkDirective::Continue)
            },
        )
        .unwrap();

        assert!(seen.iter().any(|path| path.ends_with("skip")));
        assert!(!seen.iter().any(|path| path.ends_with("skip/file.txt")));
        assert!(seen.iter().any(|path| path.ends_with("keep/file.txt")));
    }

    #[test]
    fn ordered_depth_mode_emits_directory_completion_after_descendants() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(root.path().join("dir/file.txt"), "child\n").unwrap();

        let mut seen = Vec::new();
        walk_ordered_with_backend(
            Arc::new(TestBackend),
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
                    WalkEvent::Entry(item) => {
                        let rel = item.entry.path.strip_prefix(root.path()).unwrap();
                        seen.push(format!("entry:{}", rel.display()));
                    }
                    WalkEvent::DirectoryComplete(item) => {
                        let rel = item.entry.path.strip_prefix(root.path()).unwrap();
                        seen.push(format!("done:{}", rel.display()));
                    }
                    WalkEvent::Error(error) => panic!("unexpected walk error: {error:?}"),
                }
                Ok(OrderedWalkDirective::Continue)
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

    struct TestBackend;

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
            entry.active_directory_identity(follow_mode)
        }
    }
}
