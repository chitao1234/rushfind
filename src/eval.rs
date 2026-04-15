use crate::account::{group_exists, user_exists};
use crate::ast::FileTypeFilter;
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::follow::FollowMode;
use crate::output::OutputSink;
use crate::pattern::matches_pattern;
use crate::planner::{RuntimeExpr, RuntimePredicate};
use crate::time::{Timestamp, TimestampKind};
use std::ffi::OsStr;

pub fn evaluate(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    sink: &mut dyn OutputSink,
) -> Result<bool, Diagnostic> {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                if !evaluate(item, entry, follow_mode, sink)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        RuntimeExpr::Or(left, right) => {
            if evaluate(left, entry, follow_mode, sink)? {
                Ok(true)
            } else {
                evaluate(right, entry, follow_mode, sink)
            }
        }
        RuntimeExpr::Not(inner) => Ok(!evaluate(inner, entry, follow_mode, sink)?),
        RuntimeExpr::Predicate(predicate) => evaluate_predicate(predicate, entry, follow_mode),
        RuntimeExpr::Action(action) => {
            sink.emit(*action, &entry.path)?;
            Ok(true)
        }
        RuntimeExpr::Barrier => Ok(true),
    }
}

fn evaluate_predicate(
    predicate: &RuntimePredicate,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<bool, Diagnostic> {
    match predicate {
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
        RuntimePredicate::RelativeTime(matcher) => {
            matcher.matches_timestamp_checked(entry_timestamp(entry, follow_mode, matcher.kind)?)
        }
        RuntimePredicate::Newer(matcher) => {
            Ok(matcher.matches_timestamp(entry_timestamp(entry, follow_mode, matcher.current)?))
        }
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
        TimestampKind::Change => entry.active_ctime(follow_mode),
        TimestampKind::Modification => entry.active_mtime(follow_mode),
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;
    use crate::entry::test_support::CountingReader;
    use crate::follow::FollowMode;
    use crate::output::RecordingSink;
    use crate::parser::parse_command;
    use crate::planner::{plan_command, ExecutionPlan};
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

    fn plan_for(args: &[&str]) -> ExecutionPlan {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let ast = parse_command(&argv).unwrap();
        plan_command(ast, 1).unwrap()
    }
}
