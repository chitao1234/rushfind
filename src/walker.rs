use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::planner::{TraversalOptions, TraversalOrder};
use crate::traversal_control::TraversalControl;
use std::fs::{self, FileType};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod ordered_emit;
use ordered_emit::{emit_directory_complete, emit_ordered_entry};

#[derive(Debug, Clone)]
pub enum WalkEvent {
    Entry(EntryContext),
    DirectoryComplete(EntryContext),
    Error(Diagnostic),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderedWalkDirective {
    Continue,
    Stop,
}

/// Directory identities from the root down to an entry. Descending shares the
/// chain with the parent instead of copying it, so the cost of tracking
/// ancestry is one small node per directory rather than one vector per child.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ancestry(Option<Arc<AncestryNode>>);

#[derive(Debug)]
struct AncestryNode {
    identity: FileIdentity,
    parent: Option<Arc<AncestryNode>>,
}

impl Ancestry {
    pub(crate) fn contains(&self, identity: FileIdentity) -> bool {
        let mut current = self.0.as_ref();
        while let Some(node) = current {
            if node.identity == identity {
                return true;
            }
            current = node.parent.as_ref();
        }
        false
    }

    fn with_directory(&self, identity: FileIdentity) -> Self {
        Self(Some(Arc::new(AncestryNode {
            identity,
            parent: self.0.clone(),
        })))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPath {
    pub(crate) path: PathBuf,
    pub(crate) root_path: Arc<PathBuf>,
    pub(crate) depth: usize,
    pub(crate) is_command_line_root: bool,
    pub(crate) physical_file_type_hint: Option<FileType>,
    pub(crate) ancestry: Ancestry,
    pub(crate) root_device: Option<u64>,
    pub(crate) parent_completion: Option<usize>,
}

#[derive(Debug, Clone)]
enum OrderedFrame {
    Visit(PendingPath),
    Complete(EntryContext),
}

enum OrderedFrameStep {
    Visit(PendingPath),
    Continue,
    Stop,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredChild {
    pub(crate) path: PathBuf,
    pub(crate) physical_file_type_hint: Option<FileType>,
}

pub(crate) trait WalkBackend: Send + Sync + 'static {
    fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic>;
    fn visit_children(
        &self,
        path: &Path,
        visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
    ) -> Result<(), Diagnostic>;
    fn read_children(
        &self,
        path: &Path,
    ) -> Result<(Vec<DiscoveredChild>, Vec<Diagnostic>), Diagnostic> {
        let mut children = Vec::new();
        let mut diagnostics = Vec::new();
        self.visit_children(path, &mut |item| match item {
            Ok(child) => children.push(child),
            Err(error) => diagnostics.push(error),
        })?;
        Ok((children, diagnostics))
    }
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

    fn visit_children(
        &self,
        path: &Path,
        visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
    ) -> Result<(), Diagnostic> {
        visit_children(path, visit)
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
    let mut stack = initial_ordered_stack(start_paths);

    while let Some(frame) = stack.pop() {
        let pending = match process_ordered_frame(frame, &mut emit)? {
            OrderedFrameStep::Visit(pending) => pending,
            OrderedFrameStep::Continue => continue,
            OrderedFrameStep::Stop => return Ok(()),
        };

        let entry = match backend.load_entry(&pending) {
            Ok(entry) => entry,
            Err(error) => {
                report_traversal_error(&mut emit, error, options, !pending.is_command_line_root)?;
                continue;
            }
        };

        let xargs_illegal = options.xargs_safe && !crate::runner::xargs_safe_path(&entry.path);
        if xargs_illegal {
            emit(WalkEvent::Error(crate::runner::xargs_illegal_path(
                &entry.path,
            )))?;
        }

        let control = match if xargs_illegal {
            Ok(TraversalControl::allow())
        } else {
            control(&entry)
        } {
            Ok(control) => control,
            Err(error) => {
                report_traversal_error(&mut emit, error, options, !pending.is_command_line_root)?;
                continue;
            }
        };

        let is_directory = match backend.active_directory_identity(&entry, follow_mode) {
            Ok(identity) => identity.is_some(),
            Err(error) => {
                report_traversal_error(&mut emit, error, options, !pending.is_command_line_root)?;
                continue;
            }
        };

        // Traversal-level failures such as a filesystem loop make GNU skip the
        // entry altogether: it reports the error and evaluates nothing for that
        // path, so the decision has to be made before the entry is emitted.
        let descend = match should_descend_directory(
            &pending,
            &entry,
            follow_mode,
            options,
            control,
            backend.as_ref(),
        ) {
            Ok(result) => result,
            Err(error) => {
                report_traversal_error(&mut emit, error, options, !pending.is_command_line_root)?;
                continue;
            }
        };

        if !xargs_illegal
            && emit_ordered_visit_for_order(&mut emit, options.order, is_directory, entry.clone())?
        {
            return Ok(());
        }

        let Some((child_ancestry, root_device)) = descend else {
            if emit_postorder_completion_if_needed(
                &mut emit,
                options.order,
                is_directory,
                entry.clone(),
            )? {
                return Ok(());
            }
            continue;
        };

        let (children, diagnostics) = match backend.read_children(&pending.path) {
            Ok(result) => result,
            Err(error) => {
                report_traversal_error(&mut emit, error, options, !pending.is_command_line_root)?;
                if emit_postorder_completion_if_needed(
                    &mut emit,
                    options.order,
                    is_directory,
                    entry.clone(),
                )? {
                    return Ok(());
                }
                continue;
            }
        };

        emit_ordered_errors(&mut emit, diagnostics, options)?;
        push_postorder_completion_frame(&mut stack, options.order, is_directory, entry);
        push_ordered_child_visits(&mut stack, children, &pending, child_ancestry, root_device);
    }

    Ok(())
}

fn initial_ordered_stack(start_paths: &[PathBuf]) -> Vec<OrderedFrame> {
    start_paths
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
                ancestry: Ancestry::default(),
                root_device: None,
                parent_completion: None,
            })
        })
        .collect()
}

fn process_ordered_frame<F>(
    frame: OrderedFrame,
    emit: &mut F,
) -> Result<OrderedFrameStep, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    match frame {
        OrderedFrame::Visit(pending) => Ok(OrderedFrameStep::Visit(pending)),
        OrderedFrame::Complete(entry) => {
            let stop = emit_directory_complete(emit, entry)?;
            Ok(if stop {
                OrderedFrameStep::Stop
            } else {
                OrderedFrameStep::Continue
            })
        }
    }
}

fn emit_ordered_visit_for_order<F>(
    emit: &mut F,
    order: TraversalOrder,
    is_directory: bool,
    entry: EntryContext,
) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    match order {
        TraversalOrder::PreOrder => emit_ordered_entry(emit, entry),
        TraversalOrder::DepthFirstPostOrder if !is_directory => emit_ordered_entry(emit, entry),
        TraversalOrder::DepthFirstPostOrder => Ok(false),
    }
}

fn emit_postorder_completion_if_needed<F>(
    emit: &mut F,
    order: TraversalOrder,
    is_directory: bool,
    entry: EntryContext,
) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    if order == TraversalOrder::DepthFirstPostOrder && is_directory {
        emit_directory_complete(emit, entry)
    } else {
        Ok(false)
    }
}

fn emit_ordered_errors<F>(
    emit: &mut F,
    diagnostics: Vec<Diagnostic>,
    options: TraversalOptions,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    for error in diagnostics {
        // Directory children always come from a listing, so they can race.
        report_traversal_error(emit, error, options, true)?;
    }
    Ok(())
}

fn push_postorder_completion_frame(
    stack: &mut Vec<OrderedFrame>,
    order: TraversalOrder,
    is_directory: bool,
    entry: EntryContext,
) {
    if order == TraversalOrder::DepthFirstPostOrder && is_directory {
        stack.push(OrderedFrame::Complete(entry));
    }
}

fn push_ordered_child_visits(
    stack: &mut Vec<OrderedFrame>,
    children: Vec<DiscoveredChild>,
    pending: &PendingPath,
    child_ancestry: Ancestry,
    root_device: Option<u64>,
) {
    for child in children.into_iter().rev() {
        stack.push(OrderedFrame::Visit(PendingPath {
            path: child.path,
            root_path: pending.root_path.clone(),
            depth: pending.depth + 1,
            is_command_line_root: false,
            physical_file_type_hint: child.physical_file_type_hint,
            ancestry: child_ancestry.clone(),
            root_device,
            parent_completion: None,
        }));
    }
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

type DescendDecision = Option<(Ancestry, Option<u64>)>;

pub(crate) fn should_descend_directory(
    pending: &PendingPath,
    entry: &EntryContext,
    follow_mode: FollowMode,
    options: TraversalOptions,
    control: TraversalControl,
    backend: &dyn WalkBackend,
) -> Result<DescendDecision, Diagnostic> {
    if control.prune || !should_descend(pending.depth, options.max_depth) {
        return Ok(None);
    }

    let Some(directory_identity) = backend.active_directory_identity(entry, follow_mode)? else {
        return Ok(None);
    };

    if pending.ancestry.contains(directory_identity) {
        return Err(loop_error(&pending.path));
    }

    let root_device = pending
        .root_device
        .or(Some(directory_identity.device_number()));
    if options.same_file_system
        && root_device.is_some_and(|device| directory_identity.device_number() != device)
    {
        return Ok(None);
    }

    Ok(Some((
        pending.ancestry.with_directory(directory_identity),
        root_device,
    )))
}

pub(crate) fn visit_children(
    path: &Path,
    visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
) -> Result<(), Diagnostic> {
    let read_dir = fs::read_dir(path).map_err(|error| path_error(path, error))?;
    for child in read_dir {
        match child {
            Ok(child) => visit(Ok(DiscoveredChild {
                path: child.path(),
                physical_file_type_hint: child.file_type().ok(),
            })),
            Err(error) => visit(Err(path_error(path, error))),
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

fn path_error(path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(format!("{}: {error}", path.display()), 1)
        .with_raw_os_error(error.raw_os_error())
}

/// Reports a traversal diagnostic, honouring `-ignore_readdir_race`. A path the
/// user named on the command line is never a race: it came from no listing.
fn report_traversal_error<F>(
    emit: &mut F,
    error: Diagnostic,
    options: TraversalOptions,
    raced: bool,
) -> Result<(), Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    if raced && options.ignore_readdir_race && error.is_readdir_race() {
        return Ok(());
    }

    emit(WalkEvent::Error(error)).map(|_| ())
}

fn loop_error(path: &Path) -> Diagnostic {
    Diagnostic::new(format!("filesystem loop detected at {}", path.display()), 1)
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveredChild, FsWalkBackend, OrderedWalkDirective, PendingPath, WalkBackend, WalkEvent,
        load_entry, walk_ordered, walk_ordered_with_backend,
    };
    use crate::diagnostics::Diagnostic;
    use crate::entry::{EntryContext, EntryKind};
    use crate::follow::FollowMode;
    use crate::identity::FileIdentity;
    use crate::planner::{TraversalOptions, TraversalOrder};
    use crate::platform::filesystem::{FilesystemKey, PlatformMetadataView, PlatformReader};
    use crate::time::Timestamp;
    use crate::traversal_control::TraversalControl;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::tempdir;

    #[test]
    fn fs_backend_visit_children_yields_each_child() {
        let root = tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            fs::write(root.path().join(name), "x\n").unwrap();
        }

        let mut seen = Vec::new();
        FsWalkBackend
            .visit_children(root.path(), &mut |item| {
                let child = item.unwrap();
                seen.push(child.path.file_name().unwrap().to_owned());
            })
            .unwrap();

        seen.sort();
        assert_eq!(
            seen,
            vec![
                OsString::from("a.txt"),
                OsString::from("b.txt"),
                OsString::from("c.txt"),
            ]
        );
    }

    #[test]
    fn read_children_collects_streamed_backend_output() {
        struct StreamingBackend;

        impl WalkBackend for StreamingBackend {
            fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
                load_entry(pending)
            }

            fn visit_children(
                &self,
                _path: &Path,
                visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
            ) -> Result<(), Diagnostic> {
                visit(Ok(DiscoveredChild {
                    path: PathBuf::from("child.txt"),
                    physical_file_type_hint: None,
                }));
                visit(Err(Diagnostic::new("synthetic stream error", 1)));
                Ok(())
            }

            fn active_directory_identity(
                &self,
                entry: &EntryContext,
                follow_mode: FollowMode,
            ) -> Result<Option<FileIdentity>, Diagnostic> {
                entry.active_directory_identity(follow_mode)
            }
        }

        let (children, diagnostics) = StreamingBackend.read_children(Path::new(".")).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].path, PathBuf::from("child.txt"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "synthetic stream error");
    }

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
                xargs_safe: false,
                ignore_readdir_race: false,
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
                    seen.push(item.path);
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
                xargs_safe: false,
                ignore_readdir_race: false,
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
                        let rel = item.path.strip_prefix(root.path()).unwrap();
                        seen.push(format!("entry:{}", rel.display()));
                    }
                    WalkEvent::DirectoryComplete(item) => {
                        let rel = item.path.strip_prefix(root.path()).unwrap();
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
                format!("entry:{}", Path::new("dir").join("file.txt").display()),
                format!("done:{}", Path::new("dir").display()),
                "done:".to_string(),
            ]
        );
    }

    #[test]
    fn physical_walk_does_not_descend_into_directory_reparse_points() {
        const MOUNT_POINT_REPARSE_TAG: u32 = 0xA0000003;

        #[derive(Clone)]
        struct ReparseBackend {
            entry: EntryContext,
            visited_children: Arc<AtomicBool>,
        }

        #[derive(Clone)]
        struct ReparseReader {
            physical_view: PlatformMetadataView,
            logical_view: PlatformMetadataView,
        }

        impl PlatformReader for ReparseReader {
            fn metadata_view(
                &self,
                _path: &Path,
                follow: bool,
            ) -> std::io::Result<PlatformMetadataView> {
                if follow {
                    Ok(self.logical_view.clone())
                } else {
                    Ok(self.physical_view.clone())
                }
            }

            fn read_link(&self, _path: &Path) -> std::io::Result<PathBuf> {
                Err(std::io::Error::from_raw_os_error(libc::ENOENT))
            }

            fn directory_is_empty(&self, _path: &Path) -> std::io::Result<bool> {
                Ok(false)
            }

            fn access(
                &self,
                _path: &Path,
                _mode: crate::entry::AccessMode,
            ) -> std::io::Result<bool> {
                Ok(false)
            }
        }

        impl WalkBackend for ReparseBackend {
            fn load_entry(&self, _pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
                Ok(self.entry.clone())
            }

            fn visit_children(
                &self,
                _path: &Path,
                _visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
            ) -> Result<(), Diagnostic> {
                self.visited_children.store(true, Ordering::SeqCst);
                Ok(())
            }

            fn active_directory_identity(
                &self,
                entry: &EntryContext,
                follow_mode: FollowMode,
            ) -> Result<Option<FileIdentity>, Diagnostic> {
                entry.active_directory_identity(follow_mode)
            }
        }

        let physical_view = PlatformMetadataView {
            kind: EntryKind::Directory,
            identity: Some(FileIdentity::Windows {
                volume_serial: 10,
                file_id: 20,
            }),
            size: 0,
            allocation_size: None,
            owner: None,
            group: None,
            mode_bits: None,
            flag_bits: Some(0),
            native_attributes: Some(0),
            reparse_tag: Some(MOUNT_POINT_REPARSE_TAG),
            link_count: Some(1),
            blocks_512: None,
            atime: Timestamp::new(1, 0),
            ctime: Timestamp::new(2, 0),
            mtime: Timestamp::new(3, 0),
            birth_time: Some(Timestamp::new(4, 0)),
            filesystem_key: Some(FilesystemKey::Numeric(10)),
            device_number: None,
        };
        let logical_view = PlatformMetadataView {
            kind: EntryKind::Directory,
            identity: Some(FileIdentity::Windows {
                volume_serial: 30,
                file_id: 40,
            }),
            size: 0,
            allocation_size: None,
            owner: None,
            group: None,
            mode_bits: None,
            flag_bits: Some(0),
            native_attributes: Some(0),
            reparse_tag: None,
            link_count: Some(1),
            blocks_512: None,
            atime: Timestamp::new(5, 0),
            ctime: Timestamp::new(6, 0),
            mtime: Timestamp::new(7, 0),
            birth_time: Some(Timestamp::new(8, 0)),
            filesystem_key: Some(FilesystemKey::Numeric(30)),
            device_number: None,
        };

        let backend = Arc::new(ReparseBackend {
            entry: EntryContext::new_with_reader(
                PathBuf::from("junction"),
                0,
                true,
                Arc::new(ReparseReader {
                    physical_view,
                    logical_view,
                }),
            ),
            visited_children: Arc::new(AtomicBool::new(false)),
        });

        walk_ordered_with_backend(
            backend.clone(),
            &[PathBuf::from("junction")],
            FollowMode::Physical,
            TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::PreOrder,
                xargs_safe: false,
                ignore_readdir_race: false,
            },
            |_entry| {
                Ok(TraversalControl {
                    matched: true,
                    prune: false,
                })
            },
            |_event| Ok(OrderedWalkDirective::Continue),
        )
        .unwrap();

        assert!(!backend.visited_children.load(Ordering::SeqCst));
    }

    /// Yields one child diagnostic so that `-ignore_readdir_race` handling can
    /// be exercised without racing against a real directory.
    struct VanishingChildBackend {
        error: Diagnostic,
    }

    impl WalkBackend for VanishingChildBackend {
        fn load_entry(&self, pending: &PendingPath) -> Result<EntryContext, Diagnostic> {
            load_entry(pending)
        }

        fn visit_children(
            &self,
            _path: &Path,
            visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
        ) -> Result<(), Diagnostic> {
            visit(Err(self.error.clone()));
            Ok(())
        }

        fn active_directory_identity(
            &self,
            entry: &EntryContext,
            follow_mode: FollowMode,
        ) -> Result<Option<FileIdentity>, Diagnostic> {
            entry.active_directory_identity(follow_mode)
        }
    }

    fn walk_vanishing_child(vanished: &Diagnostic, ignore_readdir_race: bool) -> Vec<String> {
        let root = tempdir().unwrap();
        let backend = Arc::new(VanishingChildBackend {
            error: vanished.clone(),
        });
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
                xargs_safe: false,
                ignore_readdir_race,
            },
            |_entry| {
                Ok(TraversalControl {
                    matched: true,
                    prune: false,
                })
            },
            |event| {
                if let WalkEvent::Error(error) = event {
                    seen.push(error.message);
                }
                Ok(OrderedWalkDirective::Continue)
            },
        )
        .unwrap();

        seen
    }

    #[test]
    fn ignore_readdir_race_hides_vanished_entries() {
        let vanished = Diagnostic::new("child: No such file or directory", 1)
            .with_raw_os_error(Some(libc::ENOENT));

        assert_eq!(walk_vanishing_child(&vanished, false).len(), 1);
        assert!(walk_vanishing_child(&vanished, true).is_empty());
    }

    #[test]
    fn ignore_readdir_race_keeps_permission_errors() {
        let denied =
            Diagnostic::new("child: Permission denied", 1).with_raw_os_error(Some(libc::EACCES));

        assert_eq!(walk_vanishing_child(&denied, false).len(), 1);
        assert_eq!(walk_vanishing_child(&denied, true).len(), 1);
    }

    #[test]
    fn command_line_roots_are_never_treated_as_a_race() {
        let missing = Diagnostic::new("root: No such file or directory", 1)
            .with_raw_os_error(Some(libc::ENOENT));

        // A root that does not exist is a real error even with the option set.
        let root = tempdir().unwrap();
        let missing_root = root.path().join("absent");
        let mut seen = Vec::new();

        walk_ordered(
            &[missing_root],
            FollowMode::Physical,
            TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::PreOrder,
                xargs_safe: false,
                ignore_readdir_race: true,
            },
            |_entry| {
                Ok(TraversalControl {
                    matched: true,
                    prune: false,
                })
            },
            |event| {
                if let WalkEvent::Error(error) = event {
                    seen.push(error.message);
                }
                Ok(OrderedWalkDirective::Continue)
            },
        )
        .unwrap();

        assert_eq!(seen.len(), 1, "expected the missing root to be reported");
        assert!(missing.is_readdir_race());
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

        fn visit_children(
            &self,
            path: &Path,
            visit: &mut dyn FnMut(Result<DiscoveredChild, Diagnostic>),
        ) -> Result<(), Diagnostic> {
            FsWalkBackend.visit_children(path, visit)
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
