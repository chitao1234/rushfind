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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RuntimeControl {
    #[default]
    Continue,
    StopRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RuntimeStatus {
    had_action_failures: bool,
    control: RuntimeControl,
}

impl RuntimeStatus {
    pub(crate) fn action_failure() -> Self {
        Self {
            had_action_failures: true,
            control: RuntimeControl::Continue,
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            had_action_failures: self.had_action_failures || other.had_action_failures,
            control: match (self.control, other.control) {
                (RuntimeControl::StopRequested, _) | (_, RuntimeControl::StopRequested) => {
                    RuntimeControl::StopRequested
                }
                _ => RuntimeControl::Continue,
            },
        }
    }

    pub(crate) fn had_action_failures(self) -> bool {
        self.had_action_failures
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionOutcome {
    pub(crate) matched: bool,
    pub(crate) status: RuntimeStatus,
}

impl ActionOutcome {
    pub(crate) fn new(matched: bool) -> Self {
        Self {
            matched,
            status: RuntimeStatus::default(),
        }
    }

    pub(crate) fn matched_true() -> Self {
        Self::new(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvalOutcome {
    pub(crate) matched: bool,
    pub(crate) status: RuntimeStatus,
}

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
    fn dispatch(&mut self, action: &RuntimeAction, path: &Path)
    -> Result<ActionOutcome, Diagnostic>;
}

pub(crate) fn evaluate_outcome_with_context(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    sink: &mut dyn ActionSink,
) -> Result<EvalOutcome, Diagnostic> {
    match expr {
        RuntimeExpr::And(items) => {
            let mut status = RuntimeStatus::default();
            for item in items {
                let outcome =
                    evaluate_outcome_with_context(item, entry, follow_mode, context, sink)?;
                status = status.merge(outcome.status);
                if !outcome.matched {
                    return Ok(EvalOutcome {
                        matched: false,
                        status,
                    });
                }
            }

            Ok(EvalOutcome {
                matched: true,
                status,
            })
        }
        RuntimeExpr::Or(left, right) => {
            let left_outcome =
                evaluate_outcome_with_context(left, entry, follow_mode, context, sink)?;
            if left_outcome.matched {
                return Ok(left_outcome);
            }

            let right_outcome =
                evaluate_outcome_with_context(right, entry, follow_mode, context, sink)?;
            Ok(EvalOutcome {
                matched: right_outcome.matched,
                status: left_outcome.status.merge(right_outcome.status),
            })
        }
        RuntimeExpr::Not(inner) => {
            let inner = evaluate_outcome_with_context(inner, entry, follow_mode, context, sink)?;
            Ok(EvalOutcome {
                matched: !inner.matched,
                status: inner.status,
            })
        }
        RuntimeExpr::Predicate(predicate) => Ok(EvalOutcome {
            matched: evaluate_predicate(predicate, entry, follow_mode, context)?,
            status: RuntimeStatus::default(),
        }),
        RuntimeExpr::Action(action) => {
            let outcome = sink.dispatch(action, &entry.path)?;
            Ok(EvalOutcome {
                matched: outcome.matched,
                status: outcome.status,
            })
        }
        RuntimeExpr::Barrier => Ok(EvalOutcome {
            matched: true,
            status: RuntimeStatus::default(),
        }),
    }
}

pub(crate) fn evaluate_with_context(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    sink: &mut dyn ActionSink,
) -> Result<bool, Diagnostic> {
    Ok(evaluate_outcome_with_context(expr, entry, follow_mode, context, sink)?.matched)
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
    use super::{
        ActionOutcome, ActionSink, EvalContext, RuntimeStatus, evaluate,
        evaluate_outcome_with_context, evaluate_with_context,
    };
    use crate::diagnostics::Diagnostic;
    use crate::entry::test_support::CountingReader;
    use crate::entry::{AccessMode, EntryContext, EntryReader};
    use crate::follow::FollowMode;
    use crate::mounts::MountSnapshot;
    use crate::output::RecordingSink;
    use crate::parser::parse_command;
    use crate::planner::{
        ExecutionPlan, OutputAction, RuntimeAction, RuntimeExpr, RuntimePredicate, plan_command,
    };
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs as unix_fs;
    use tempfile::tempdir;

    #[derive(Default)]
    struct ScriptedSink {
        scripted: VecDeque<ActionOutcome>,
    }

    impl ActionSink for ScriptedSink {
        fn dispatch(
            &mut self,
            _action: &RuntimeAction,
            _path: &std::path::Path,
        ) -> Result<ActionOutcome, Diagnostic> {
            self.scripted
                .pop_front()
                .ok_or_else(|| Diagnostic::new("test sink ran out of scripted outcomes", 1))
        }
    }

    #[test]
    fn action_failure_status_survives_and_short_circuit() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::And(vec![
            RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
            RuntimeExpr::Predicate(RuntimePredicate::True),
        ]);
        let mut sink = ScriptedSink {
            scripted: VecDeque::from([ActionOutcome {
                matched: false,
                status: RuntimeStatus::action_failure(),
            }]),
        };

        let outcome = evaluate_outcome_with_context(
            &expr,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
            &mut sink,
        )
        .unwrap();

        assert!(!outcome.matched);
        assert!(outcome.status.had_action_failures());
    }

    #[test]
    fn or_keeps_status_from_false_left_branch_before_true_right_branch() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::Or(
            Box::new(RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print))),
            Box::new(RuntimeExpr::Predicate(RuntimePredicate::True)),
        );
        let mut sink = ScriptedSink {
            scripted: VecDeque::from([ActionOutcome {
                matched: false,
                status: RuntimeStatus::action_failure(),
            }]),
        };

        let outcome = evaluate_outcome_with_context(
            &expr,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
            &mut sink,
        )
        .unwrap();

        assert!(outcome.matched);
        assert!(outcome.status.had_action_failures());
    }

    #[test]
    fn not_inverts_truth_without_dropping_status() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::Not(Box::new(RuntimeExpr::Action(RuntimeAction::Output(
            OutputAction::Print,
        ))));
        let mut sink = ScriptedSink {
            scripted: VecDeque::from([ActionOutcome {
                matched: false,
                status: RuntimeStatus::action_failure(),
            }]),
        };

        let outcome = evaluate_outcome_with_context(
            &expr,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
            &mut sink,
        )
        .unwrap();

        assert!(outcome.matched);
        assert!(outcome.status.had_action_failures());
    }

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
