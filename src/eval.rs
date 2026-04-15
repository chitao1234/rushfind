use crate::account::{group_exists, user_exists};
use crate::ast::FileTypeFilter;
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::follow::FollowMode;
use crate::output::OutputSink;
use crate::pattern::matches_pattern;
use crate::planner::{RuntimeExpr, RuntimePredicate};
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
        RuntimeExpr::TraversalBoundary => Ok(true),
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
        RuntimePredicate::Inum(expected) => Ok(expected.matches(entry.active_inode(follow_mode))),
        RuntimePredicate::Links(expected) => {
            Ok(expected.matches(entry.active_link_count(follow_mode)))
        }
        RuntimePredicate::SameFile(expected) => Ok(*expected == entry.active_identity(follow_mode)),
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
        RuntimePredicate::Uid(expected) => Ok(expected.matches(entry.active_uid(follow_mode).into())),
        RuntimePredicate::Gid(expected) => Ok(expected.matches(entry.active_gid(follow_mode).into())),
        RuntimePredicate::User(expected) => Ok(*expected == entry.active_uid(follow_mode)),
        RuntimePredicate::Group(expected) => Ok(*expected == entry.active_gid(follow_mode)),
        RuntimePredicate::NoUser => Ok(!user_exists(entry.active_uid(follow_mode))?),
        RuntimePredicate::NoGroup => Ok(!group_exists(entry.active_gid(follow_mode))?),
        RuntimePredicate::Perm(matcher) => Ok(matcher.matches(entry.active_mode_bits(follow_mode))),
        RuntimePredicate::Type(expected) => {
            Ok(matches_type(*expected, entry.active_kind(follow_mode)))
        }
        RuntimePredicate::XType(expected) => {
            Ok(matches_type(*expected, entry.xtype_kind(follow_mode)))
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
