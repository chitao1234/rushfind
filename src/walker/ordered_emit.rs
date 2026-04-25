use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::runtime_pipeline::SubtreeBarrierId;

use super::{OrderedWalkDirective, WalkEvent, scheduled_entry};

pub(super) fn emit_ordered_entry<F>(
    emit: &mut F,
    entry: EntryContext,
    sequence: &mut u64,
    ancestor_barriers: Vec<SubtreeBarrierId>,
) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    let directive = emit(WalkEvent::Entry(scheduled_entry(
        entry,
        *sequence,
        ancestor_barriers,
        None,
    )))?;
    *sequence += 1;
    Ok(directive == OrderedWalkDirective::Stop)
}

pub(super) fn emit_directory_complete<F>(
    emit: &mut F,
    entry: EntryContext,
    sequence: &mut u64,
    ancestor_barriers: Vec<SubtreeBarrierId>,
) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    let directive = emit(WalkEvent::DirectoryComplete(scheduled_entry(
        entry,
        *sequence,
        ancestor_barriers,
        None,
    )))?;
    *sequence += 1;
    Ok(directive == OrderedWalkDirective::Stop)
}
