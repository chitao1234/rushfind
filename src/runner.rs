use crate::diagnostics::Diagnostic;
use crate::eval::{EvalContext, evaluate_with_context};
use crate::mounts::MountSnapshot;
use crate::output::{BrokerMessage, spawn_broker};
use crate::planner::{ExecutionMode, ExecutionPlan};
use crate::traversal_control::evaluate_for_traversal_with_context;
use crate::walker::{WalkEvent, walk_ordered, walk_parallel};
use crossbeam_channel::unbounded;
use std::io::Write;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    pub had_runtime_errors: bool,
    pub had_action_failures: bool,
}

pub fn run_plan<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<RunSummary, Diagnostic>
where
    W: Write + Send,
    E: Write + Send,
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
    let eval_context = build_eval_context(plan)?;
    let mut sink = crate::exec::OrderedActionSink::new(stdout, stderr);
    let mut had_runtime_errors = false;

    walk_ordered(
        &plan.start_paths,
        plan.follow_mode,
        plan.traversal,
        |entry| {
            evaluate_for_traversal_with_context(
                &plan.expr,
                entry,
                plan.follow_mode,
                plan.traversal.order,
                &eval_context,
            )
        },
        |event| {
            match event {
                WalkEvent::Entry(entry) | WalkEvent::DirectoryComplete(entry) => {
                    if entry.depth >= plan.traversal.min_depth {
                        let _ = evaluate_with_context(
                            &plan.expr,
                            &entry,
                            plan.follow_mode,
                            &eval_context,
                            &mut sink,
                        )?;
                    }
                }
                WalkEvent::Error(error) => {
                    had_runtime_errors = true;
                    sink.write_diagnostic(&format!("findoxide: {error}"))?;
                }
            }
            Ok(())
        },
    )?;

    let had_action_failures = sink.flush()?;

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
    })
}

fn run_parallel<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<RunSummary, Diagnostic>
where
    W: Write + Send,
    E: Write + Send,
{
    let eval_context = build_eval_context(plan)?;
    let worker_count = std::env::var("FINDOXIDE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let evaluator_count =
        if plan.traversal.order == crate::planner::TraversalOrder::DepthFirstPostOrder {
            1
        } else {
            worker_count
        };
    let mut had_runtime_errors = false;
    let mut had_action_failures = false;

    std::thread::scope(|scope| -> Result<(), Diagnostic> {
        let (broker, broker_handle) = spawn_broker(scope, stdout, stderr);
        let (entry_tx, entry_rx) = unbounded();
        let sink = crate::exec::ParallelActionSink::new(broker.clone(), worker_count)?;

        let mut evaluators = Vec::new();
        for _ in 0..evaluator_count {
            let entry_rx = entry_rx.clone();
            let expr = plan.expr.clone();
            let eval_context = eval_context.clone();
            let mut sink = sink.clone();
            let follow_mode = plan.follow_mode;
            evaluators.push(scope.spawn(move || -> Result<(), Diagnostic> {
                while let Ok(entry) = entry_rx.recv() {
                    let _ = evaluate_with_context(
                        &expr,
                        &entry,
                        follow_mode,
                        &eval_context,
                        &mut sink,
                    )?;
                }
                Ok(())
            }));
        }
        drop(entry_rx);

        let control_expr = plan.expr.clone();
        let control_context = eval_context.clone();
        let follow_mode = plan.follow_mode;
        let traversal_order = plan.traversal.order;
        for event in walk_parallel(
            &plan.start_paths,
            plan.follow_mode,
            plan.traversal,
            worker_count,
            move |entry| {
                evaluate_for_traversal_with_context(
                    &control_expr,
                    entry,
                    follow_mode,
                    traversal_order,
                    &control_context,
                )
            },
        ) {
            match event {
                WalkEvent::Entry(entry) | WalkEvent::DirectoryComplete(entry) => {
                    if entry.depth >= plan.traversal.min_depth {
                        entry_tx.send(entry).map_err(|_| {
                            Diagnostic::new(
                                "internal error: parallel evaluator channel is unavailable",
                                1,
                            )
                        })?;
                    }
                }
                WalkEvent::Error(error) => {
                    had_runtime_errors = true;
                    broker
                        .send(BrokerMessage::Stderr(
                            format!("findoxide: {error}\n").into_bytes(),
                        ))
                        .map_err(|_| {
                            Diagnostic::new("internal error: output broker is unavailable", 1)
                        })?;
                }
            }
        }

        drop(entry_tx);
        for handle in evaluators {
            handle
                .join()
                .map_err(|_| Diagnostic::new("parallel evaluator thread panicked", 1))??;
        }

        had_action_failures = sink.flush_all()?;
        drop(sink);
        drop(broker);
        broker_handle
            .join()
            .map_err(|_| Diagnostic::new("output broker thread panicked", 1))??;
        Ok(())
    })?;

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
    })
}

fn build_eval_context(plan: &ExecutionPlan) -> Result<EvalContext, Diagnostic> {
    build_eval_context_with_loader(plan, MountSnapshot::load_proc_self_mountinfo)
}

fn build_eval_context_with_loader<F>(
    plan: &ExecutionPlan,
    load_mount_snapshot: F,
) -> Result<EvalContext, Diagnostic>
where
    F: FnOnce() -> Result<MountSnapshot, Diagnostic>,
{
    if !plan.runtime.mount_snapshot {
        return Ok(EvalContext::default());
    }

    Ok(EvalContext::with_mount_snapshot(load_mount_snapshot()?))
}

#[cfg(test)]
mod tests {
    use super::build_eval_context_with_loader;
    use crate::follow::FollowMode;
    use crate::planner::{
        ExecutionMode, ExecutionPlan, OutputAction, RuntimeAction, RuntimeExpr,
        RuntimeRequirements, TraversalOptions, TraversalOrder,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn plan_with_runtime(mount_snapshot: bool) -> ExecutionPlan {
        ExecutionPlan {
            start_paths: vec![PathBuf::from(".")],
            follow_mode: FollowMode::Physical,
            traversal: TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::PreOrder,
            },
            runtime: RuntimeRequirements { mount_snapshot },
            expr: RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
            mode: ExecutionMode::OrderedSingle,
        }
    }

    #[test]
    fn mount_snapshot_loader_is_skipped_when_plan_does_not_require_it() {
        let calls = AtomicUsize::new(0);

        let _ = build_eval_context_with_loader(&plan_with_runtime(false), || {
            calls.fetch_add(1, Ordering::SeqCst);
            panic!("mount snapshot loader should not run");
        })
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mount_snapshot_loader_runs_once_when_plan_requires_it() {
        let calls = AtomicUsize::new(0);

        let _ = build_eval_context_with_loader(&plan_with_runtime(true), || {
            calls.fetch_add(1, Ordering::SeqCst);
            crate::mounts::MountSnapshot::from_mountinfo("1 0 8:1 / / rw - tmpfs tmpfs rw\n")
        })
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
