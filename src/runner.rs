use crate::diagnostics::Diagnostic;
use crate::eval::evaluate;
use crate::output::StdoutSink;
use crate::planner::{ExecutionMode, ExecutionPlan};
use crate::traversal_control::evaluate_for_traversal;
use crate::walker::{WalkEvent, walk_ordered, walk_parallel};
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
        ExecutionMode::ParallelRelaxed => run_parallel(plan, stdout, stderr),
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

    walk_ordered(
        &plan.start_paths,
        plan.follow_mode,
        plan.traversal,
        |entry| evaluate_for_traversal(&plan.expr, entry, plan.follow_mode),
        |event| {
            match event {
                WalkEvent::Entry(entry) => {
                    if entry.depth >= plan.traversal.min_depth {
                        let _ = evaluate(&plan.expr, &entry, plan.follow_mode, &mut sink)?;
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
        },
    )?;

    Ok(RunSummary { had_runtime_errors })
}

fn run_parallel<W, E>(
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
    let control_expr = plan.expr.clone();
    let follow_mode = plan.follow_mode;
    let worker_count = std::env::var("FINDOXIDE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);

    for event in walk_parallel(
        &plan.start_paths,
        plan.follow_mode,
        plan.traversal,
        worker_count,
        move |entry| evaluate_for_traversal(&control_expr, entry, follow_mode),
    ) {
        match event {
            WalkEvent::Entry(entry) => {
                if entry.depth >= plan.traversal.min_depth {
                    let _ = evaluate(&plan.expr, &entry, plan.follow_mode, &mut sink)?;
                }
            }
            WalkEvent::Error(error) => {
                had_runtime_errors = true;
                writeln!(stderr, "findoxide: {}", error).map_err(|io_error| {
                    Diagnostic::new(format!("failed to write stderr: {io_error}"), 1)
                })?;
            }
        }
    }

    Ok(RunSummary { had_runtime_errors })
}
