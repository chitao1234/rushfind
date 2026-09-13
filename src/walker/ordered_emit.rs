use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;

use super::{OrderedWalkDirective, WalkEvent};

pub(super) fn emit_ordered_entry<F>(emit: &mut F, entry: EntryContext) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    let directive = emit(WalkEvent::Entry(entry))?;
    Ok(directive == OrderedWalkDirective::Stop)
}

pub(super) fn emit_directory_complete<F>(
    emit: &mut F,
    entry: EntryContext,
) -> Result<bool, Diagnostic>
where
    F: FnMut(WalkEvent) -> Result<OrderedWalkDirective, Diagnostic>,
{
    let directive = emit(WalkEvent::DirectoryComplete(entry))?;
    Ok(directive == OrderedWalkDirective::Stop)
}
