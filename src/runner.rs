use crate::diagnostics::Diagnostic;
use crate::eval::evaluate;
use crate::output::StdoutSink;
use crate::planner::{ExecutionMode, ExecutionPlan};
use crate::walker::{walk_ordered, WalkEvent};
use std::io::Write;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    pub had_runtime_errors: bool,
}

pub fn run_plan<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    match plan.mode {
        ExecutionMode::OrderedSingle => run_ordered(plan, stdout, stderr),
        ExecutionMode::ParallelRelaxed => {
            writeln!(stderr, "findoxide: parallel execution not implemented yet")
                .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))?;
            Ok(RunSummary {
                had_runtime_errors: true,
            })
        }
    }
}

fn run_ordered<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let mut sink = StdoutSink::new(stdout);
    let mut had_runtime_errors = false;

    walk_ordered(&plan.start_paths, plan.traversal, |event| {
        match event {
            WalkEvent::Entry(entry) => {
                if entry.depth >= plan.traversal.min_depth {
                    let _ = evaluate(&plan.expr, &entry, &mut sink)?;
                }
            }
            WalkEvent::Error(error) => {
                had_runtime_errors = true;
                writeln!(stderr, "findoxide: {}", error).map_err(|io_error| {
                    Diagnostic::new(format!("failed to write stderr: {io_error}"), 1)
                })?;
            }
        }
        Ok(())
    })?;

    Ok(RunSummary { had_runtime_errors })
}
