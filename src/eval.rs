use crate::ast::FileTypeFilter;
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::output::OutputSink;
use crate::pattern::matches_pattern;
use crate::planner::{RuntimeExpr, RuntimePredicate};
use std::ffi::OsStr;

pub fn evaluate(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    sink: &mut dyn OutputSink,
) -> Result<bool, Diagnostic> {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                if !evaluate(item, entry, sink)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        RuntimeExpr::Or(left, right) => {
            if evaluate(left, entry, sink)? {
                Ok(true)
            } else {
                evaluate(right, entry, sink)
            }
        }
        RuntimeExpr::Not(inner) => Ok(!evaluate(inner, entry, sink)?),
        RuntimeExpr::Predicate(predicate) => evaluate_predicate(predicate, entry),
        RuntimeExpr::Action(action) => {
            sink.emit(*action, &entry.path)?;
            Ok(true)
        }
    }
}

fn evaluate_predicate(
    predicate: &RuntimePredicate,
    entry: &EntryContext,
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
        RuntimePredicate::Type(expected) => Ok(matches_type(*expected, entry.kind)),
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
