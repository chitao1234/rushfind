use crate::account::{group_exists, user_exists};
use crate::ast::FileTypeFilter;
use crate::diagnostics::Diagnostic;
use crate::entry::{AccessMode, EntryContext, EntryKind};
use crate::follow::FollowMode;
use crate::mounts::MountSnapshot;
use crate::pattern::matches_pattern;
use crate::planner::{RuntimeAction, RuntimeExpr, RuntimePredicate};
use crate::time::{NewerMatcher, Timestamp, TimestampKind};
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub(crate) struct EvalContext {
    mount_snapshot: Option<Arc<MountSnapshot>>,
}

impl EvalContext {
    pub(crate) fn with_mount_snapshot(snapshot: MountSnapshot) -> Self {
        Self {
            mount_snapshot: Some(Arc::new(snapshot)),
        }
    }

    fn mount_snapshot(&self) -> Result<&MountSnapshot, Diagnostic> {
        self.mount_snapshot.as_deref().ok_or_else(|| {
            Diagnostic::new(
                "internal error: -fstype requires a mount snapshot runtime context",
                1,
            )
        })
    }
}

pub fn evaluate(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    sink: &mut dyn ActionSink,
) -> Result<bool, Diagnostic> {
    let context = EvalContext::default();
    evaluate_with_context(expr, entry, follow_mode, &context, sink)
}

pub trait ActionSink {
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path) -> Result<bool, Diagnostic>;
}

pub(crate) fn evaluate_with_context(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    sink: &mut dyn ActionSink,
) -> Result<bool, Diagnostic> {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                if !evaluate_with_context(item, entry, follow_mode, context, sink)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        RuntimeExpr::Or(left, right) => {
            if evaluate_with_context(left, entry, follow_mode, context, sink)? {
                Ok(true)
            } else {
                evaluate_with_context(right, entry, follow_mode, context, sink)
            }
        }
        RuntimeExpr::Not(inner) => Ok(!evaluate_with_context(
            inner,
            entry,
            follow_mode,
            context,
            sink,
        )?),
        RuntimeExpr::Predicate(predicate) => {
            evaluate_predicate(predicate, entry, follow_mode, context)
        }
        RuntimeExpr::Action(action) => sink.dispatch(action, &entry.path),
        RuntimeExpr::Barrier => Ok(true),
    }
}

pub(crate) fn evaluate_predicate(
    predicate: &RuntimePredicate,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<bool, Diagnostic> {
    match predicate {
        RuntimePredicate::Prune => Ok(true),
        RuntimePredicate::FsType(type_name) => {
            let snapshot = context.mount_snapshot()?;
            if !snapshot.knows_type(type_name.as_os_str()) {
                return Ok(false);
            }

            let mount_id = entry.active_mount_id(follow_mode)?;
            Ok(snapshot
                .type_for_mount_id(mount_id)
                .is_some_and(|actual| actual == type_name.as_os_str()))
        }
        RuntimePredicate::Readable => entry.access(AccessMode::Read),
        RuntimePredicate::Writable => entry.access(AccessMode::Write),
        RuntimePredicate::Executable => entry.access(AccessMode::Execute),
        RuntimePredicate::Name {
            pattern,
            case_insensitive,
        } => {
            let basename = entry.path.file_name().unwrap_or_else(|| OsStr::new(""));
            matches_pattern(pattern.as_os_str(), basename, *case_insensitive, false)
        }
        RuntimePredicate::Path {
            pattern,
            case_insensitive,
        } => matches_pattern(
            pattern.as_os_str(),
            entry.path.as_os_str(),
            *case_insensitive,
            true,
        ),
        RuntimePredicate::Inum(expected) => Ok(expected.matches(entry.active_inode(follow_mode)?)),
        RuntimePredicate::Links(expected) => {
            Ok(expected.matches(entry.active_link_count(follow_mode)?))
        }
        RuntimePredicate::SameFile(expected) => {
            Ok(*expected == entry.active_identity(follow_mode)?)
        }
        RuntimePredicate::LName {
            pattern,
            case_insensitive,
        } => match entry.active_link_target(follow_mode)? {
            Some(target) => matches_pattern(
                pattern.as_os_str(),
                target.as_os_str(),
                *case_insensitive,
                false,
            ),
            None => Ok(false),
        },
        RuntimePredicate::Uid(expected) => {
            Ok(expected.matches(entry.active_uid(follow_mode)?.into()))
        }
        RuntimePredicate::Gid(expected) => {
            Ok(expected.matches(entry.active_gid(follow_mode)?.into()))
        }
        RuntimePredicate::User(expected) => Ok(*expected == entry.active_uid(follow_mode)?),
        RuntimePredicate::Group(expected) => Ok(*expected == entry.active_gid(follow_mode)?),
        RuntimePredicate::NoUser => Ok(!user_exists(entry.active_uid(follow_mode)?)?),
        RuntimePredicate::NoGroup => Ok(!group_exists(entry.active_gid(follow_mode)?)?),
        RuntimePredicate::Perm(matcher) => {
            Ok(matcher.matches(entry.active_mode_bits(follow_mode)?))
        }
        RuntimePredicate::Size(matcher) => Ok(matcher.matches(entry.active_size(follow_mode)?)),
        RuntimePredicate::Empty => entry.active_is_empty(follow_mode),
        RuntimePredicate::Used(matcher) => Ok(matcher.matches(
            entry.active_atime(follow_mode)?,
            entry.active_ctime(follow_mode)?,
        )),
        RuntimePredicate::RelativeTime(matcher) => {
            matcher.matches_timestamp_checked(entry_timestamp(entry, follow_mode, matcher.kind)?)
        }
        RuntimePredicate::Newer(matcher) => matches_newer(entry, follow_mode, *matcher),
        RuntimePredicate::Type(expected) => {
            Ok(matches_type(*expected, entry.active_kind(follow_mode)?))
        }
        RuntimePredicate::XType(expected) => {
            Ok(matches_type(*expected, entry.xtype_kind(follow_mode)?))
        }
        RuntimePredicate::True => Ok(true),
        RuntimePredicate::False => Ok(false),
    }
}

fn matches_type(expected: FileTypeFilter, actual: EntryKind) -> bool {
    matches!(
        (expected, actual),
        (FileTypeFilter::File, EntryKind::File)
            | (FileTypeFilter::Directory, EntryKind::Directory)
            | (FileTypeFilter::Symlink, EntryKind::Symlink)
            | (FileTypeFilter::Block, EntryKind::Block)
            | (FileTypeFilter::Character, EntryKind::Character)
            | (FileTypeFilter::Fifo, EntryKind::Fifo)
            | (FileTypeFilter::Socket, EntryKind::Socket)
    )
}

fn entry_timestamp(
    entry: &EntryContext,
    follow_mode: FollowMode,
    kind: TimestampKind,
) -> Result<Timestamp, Diagnostic> {
    match kind {
        TimestampKind::Access => entry.active_atime(follow_mode),
        TimestampKind::Birth => unreachable!("birth timestamps are handled separately"),
        TimestampKind::Change => entry.active_ctime(follow_mode),
        TimestampKind::Modification => entry.active_mtime(follow_mode),
    }
}

fn matches_newer(
    entry: &EntryContext,
    follow_mode: FollowMode,
    matcher: NewerMatcher,
) -> Result<bool, Diagnostic> {
    if matcher.current == TimestampKind::Birth {
        return Ok(entry
            .active_birth_time(follow_mode)?
            .is_some_and(|actual| matcher.matches_timestamp(actual)));
    }

    Ok(matcher.matches_timestamp(entry_timestamp(entry, follow_mode, matcher.current)?))
}

#[cfg(test)]
mod tests {
    use super::{EvalContext, evaluate, evaluate_with_context};
    use crate::entry::test_support::CountingReader;
    use crate::entry::{AccessMode, EntryContext, EntryReader};
    use crate::follow::FollowMode;
    use crate::mounts::MountSnapshot;
    use crate::output::RecordingSink;
    use crate::parser::parse_command;
    use crate::planner::{ExecutionPlan, RuntimeExpr, RuntimePredicate, plan_command};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs as unix_fs;
    use tempfile::tempdir;

    #[test]
    fn planned_name_mismatch_skips_later_metadata_predicate() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let plan = plan_for(&[".", "-uid", "0", "-name", "does-not-match"]);
        let mut sink = RecordingSink::default();

        assert!(!evaluate(&plan.expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert_eq!(reader.symlink_metadata_calls(), 0);
        assert_eq!(reader.metadata_calls(), 0);
        assert_eq!(reader.read_link_calls(), 0);
        assert_eq!(sink.into_utf8(), "");
    }

    #[test]
    fn planned_name_mismatch_skips_later_link_target_predicate() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("real.txt"), "hello\n").unwrap();
        unix_fs::symlink("real.txt", root.path().join("file-link")).unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(root.path().join("file-link"), 0, true);
        let plan = plan_for(&[".", "-lname", "*", "-name", "does-not-match"]);
        let mut sink = RecordingSink::default();

        assert!(!evaluate(&plan.expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert_eq!(reader.symlink_metadata_calls(), 0);
        assert_eq!(reader.metadata_calls(), 0);
        assert_eq!(reader.read_link_calls(), 0);
        assert_eq!(sink.into_utf8(), "");
    }

    #[test]
    fn planned_name_mismatch_skips_later_directory_probe() {
        let root = tempdir().unwrap();
        let path = root.path().join("empty-dir");
        fs::create_dir(&path).unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let plan = plan_for(&[".", "-empty", "-name", "does-not-match"]);
        let mut sink = RecordingSink::default();

        assert!(!evaluate(&plan.expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert_eq!(reader.symlink_metadata_calls(), 0);
        assert_eq!(reader.metadata_calls(), 0);
        assert_eq!(reader.read_link_calls(), 0);
        assert_eq!(reader.directory_probe_calls(), 0);
    }

    #[test]
    fn empty_directory_probe_is_loaded_once() {
        let root = tempdir().unwrap();
        let path = root.path().join("empty-dir");
        fs::create_dir(&path).unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let expr = RuntimeExpr::Predicate(RuntimePredicate::Empty);
        let mut sink = RecordingSink::default();

        assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert!(evaluate(&expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert_eq!(reader.directory_probe_calls(), 1);
    }

    #[test]
    fn planned_name_mismatch_skips_later_access_probe() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let plan = plan_for(&[".", "-readable", "-name", "does-not-match"]);
        let mut sink = RecordingSink::default();

        assert!(!evaluate(&plan.expr, &entry, FollowMode::Physical, &mut sink).unwrap());
        assert_eq!(reader.read_access_calls(), 0);
        assert_eq!(sink.into_utf8(), "");
    }

    #[test]
    fn runtime_access_predicates_dispatch_by_mode() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new_with_reader(
            path,
            0,
            true,
            std::sync::Arc::new(AccessResultReader {
                readable: true,
                writable: false,
                executable: true,
            }),
        );
        let mut sink = RecordingSink::default();

        assert!(
            evaluate(
                &RuntimeExpr::Predicate(RuntimePredicate::Readable),
                &entry,
                FollowMode::Physical,
                &mut sink,
            )
            .unwrap()
        );
        assert!(
            !evaluate(
                &RuntimeExpr::Predicate(RuntimePredicate::Writable),
                &entry,
                FollowMode::Physical,
                &mut sink,
            )
            .unwrap()
        );
        assert!(
            evaluate(
                &RuntimeExpr::Predicate(RuntimePredicate::Executable),
                &entry,
                FollowMode::Physical,
                &mut sink,
            )
            .unwrap()
        );
    }

    #[test]
    fn fstype_matches_mount_snapshot_type() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let mount_id = entry.active_mount_id(FollowMode::Physical).unwrap();
        let context = EvalContext::with_mount_snapshot(
            MountSnapshot::from_mountinfo(&format!("{mount_id} 1 8:1 / / rw - tmpfs tmpfs rw\n"))
                .unwrap(),
        );
        let mut sink = RecordingSink::default();

        assert!(
            evaluate_with_context(
                &RuntimeExpr::Predicate(RuntimePredicate::FsType("tmpfs".into())),
                &entry,
                FollowMode::Physical,
                &context,
                &mut sink,
            )
            .unwrap()
        );
        assert!(
            !evaluate_with_context(
                &RuntimeExpr::Predicate(RuntimePredicate::FsType("ext4".into())),
                &entry,
                FollowMode::Physical,
                &context,
                &mut sink,
            )
            .unwrap()
        );
    }

    #[test]
    fn fstype_without_mount_snapshot_context_is_a_runtime_error() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let mut sink = RecordingSink::default();

        let error = evaluate_with_context(
            &RuntimeExpr::Predicate(RuntimePredicate::FsType("tmpfs".into())),
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
            &mut sink,
        )
        .unwrap_err();

        assert!(error.message.contains("mount snapshot"));
    }

    #[derive(Clone)]
    struct AccessResultReader {
        readable: bool,
        writable: bool,
        executable: bool,
    }

    impl EntryReader for AccessResultReader {
        fn symlink_metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
            std::fs::symlink_metadata(path)
        }

        fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
            std::fs::metadata(path)
        }

        fn read_link(&self, path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
            std::fs::read_link(path)
        }

        fn directory_is_empty(&self, path: &std::path::Path) -> std::io::Result<bool> {
            let mut entries = std::fs::read_dir(path)?;
            match entries.next() {
                None => Ok(true),
                Some(result) => result.map(|_| false),
            }
        }

        fn mount_id(&self, path: &std::path::Path, follow: bool) -> std::io::Result<u64> {
            let _ = (path, follow);
            Ok(0)
        }

        fn access(&self, _path: &std::path::Path, mode: AccessMode) -> std::io::Result<bool> {
            Ok(match mode {
                AccessMode::Read => self.readable,
                AccessMode::Write => self.writable,
                AccessMode::Execute => self.executable,
            })
        }
    }

    fn plan_for(args: &[&str]) -> ExecutionPlan {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let ast = parse_command(&argv).unwrap();
        plan_command(ast, 1).unwrap()
    }
}
