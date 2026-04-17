use crate::diagnostics::Diagnostic;
use crate::eval::{ActionSink, EvalContext, RuntimeStatus, evaluate_with_context};
use crate::mounts::MountSnapshot;
use crate::output::{BrokerMessage, spawn_broker};
use crate::planner::{ExecutionMode, ExecutionPlan, RuntimeExpr, TraversalOrder};
use crate::runtime_pipeline::{
    EntryTicket, EvalStep, OrderedReadyQueue, SubtreeBarrierTracker, begin_entry_eval,
    resume_entry_eval,
};
use crate::traversal_control::{TraversalControl, evaluate_for_traversal_with_context};
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

fn traversal_control_for_entry(
    expr: Option<&RuntimeExpr>,
    follow_mode: crate::follow::FollowMode,
    order: TraversalOrder,
    entry: &crate::entry::EntryContext,
    context: &EvalContext,
) -> Result<TraversalControl, Diagnostic> {
    match expr {
        Some(expr) => evaluate_for_traversal_with_context(expr, entry, follow_mode, order, context),
        None => Ok(TraversalControl::allow()),
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
    if contains_commit_sensitive_action(&plan.expr) {
        return run_ordered_inline(plan, stdout, stderr);
    }

    run_ordered_pipeline(plan, stdout, stderr)
}

fn run_ordered_inline<W, E>(
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
            traversal_control_for_entry(
                plan.traversal_control.as_ref(),
                plan.follow_mode,
                plan.traversal.order,
                entry,
                &eval_context,
            )
        },
        |event| {
            match event {
                WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
                    let entry = item.entry;
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

    let had_action_failures = sink.flush()?.had_action_failures();

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
    })
}

fn run_ordered_pipeline<W, E>(
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
    let mut had_action_failures = false;

    std::thread::scope(|scope| -> Result<(), Diagnostic> {
        let workers = ordered_evaluator_workers(plan);
        let (work_tx, work_rx) = unbounded::<(u64, crate::entry::EntryContext)>();
        let (ready_tx, ready_rx) = unbounded::<(u64, Result<EvalStep, Diagnostic>)>();

        let mut evaluators = Vec::new();
        for _ in 0..workers {
            let work_rx = work_rx.clone();
            let ready_tx = ready_tx.clone();
            let expr = plan.expr.clone();
            let eval_context = eval_context.clone();
            let follow_mode = plan.follow_mode;
            evaluators.push(scope.spawn(move || -> Result<(), Diagnostic> {
                while let Ok((sequence, entry)) = work_rx.recv() {
                    let step = begin_entry_eval(&expr, &entry, follow_mode, &eval_context);
                    ready_tx.send((sequence, step)).map_err(|_| {
                        Diagnostic::new("internal error: ordered ready queue is unavailable", 1)
                    })?;
                }
                Ok(())
            }));
        }
        drop(work_rx);
        drop(ready_tx);

        let mut next_sequence = 0_u64;
        walk_ordered(
            &plan.start_paths,
            plan.follow_mode,
            plan.traversal,
            |entry| {
                traversal_control_for_entry(
                    plan.traversal_control.as_ref(),
                    plan.follow_mode,
                    plan.traversal.order,
                    entry,
                    &eval_context,
                )
            },
            |event| {
                match event {
                    WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
                        let entry = item.entry;
                        if entry.depth >= plan.traversal.min_depth {
                            work_tx.send((next_sequence, entry)).map_err(|_| {
                                Diagnostic::new(
                                    "internal error: ordered evaluator channel is unavailable",
                                    1,
                                )
                            })?;
                            next_sequence += 1;
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
        drop(work_tx);

        let mut queue = OrderedReadyQueue::default();
        let mut status = RuntimeStatus::default();
        for (sequence, step) in ready_rx {
            queue.insert(sequence, step);
            while let Some(ready) = queue.pop_next() {
                let mut ready = ready?;
                loop {
                    match ready {
                        EvalStep::Complete(outcome) => {
                            status = status.merge(outcome.status);
                            break;
                        }
                        EvalStep::PendingAction {
                            request,
                            continuation,
                        } => {
                            let outcome = sink.dispatch(
                                request.action(),
                                request.entry(),
                                request.follow_mode(),
                            )?;
                            ready = resume_entry_eval(continuation, outcome, &eval_context)?;
                        }
                    }
                }
            }
        }

        for handle in evaluators {
            handle
                .join()
                .map_err(|_| Diagnostic::new("ordered evaluator thread panicked", 1))??;
        }

        status = status.merge(sink.flush()?);
        had_action_failures = status.had_action_failures();
        Ok(())
    })?;

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
    })
}

fn ordered_evaluator_workers(plan: &ExecutionPlan) -> usize {
    if contains_commit_sensitive_action(&plan.expr) {
        1
    } else {
        plan.runtime_policy.evaluation_workers
    }
}

fn contains_commit_sensitive_action(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_commit_sensitive_action),
        RuntimeExpr::Or(left, right) => {
            contains_commit_sensitive_action(left) || contains_commit_sensitive_action(right)
        }
        RuntimeExpr::Not(inner) => contains_commit_sensitive_action(inner),
        RuntimeExpr::Action(crate::planner::RuntimeAction::Output(_)) => false,
        RuntimeExpr::Action(crate::planner::RuntimeAction::Printf(_)) => false,
        RuntimeExpr::Action(_) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => false,
    }
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
    let worker_count = plan.runtime_policy.requested_workers;
    let mut had_runtime_errors = false;
    let mut had_action_failures = false;

    std::thread::scope(|scope| -> Result<(), Diagnostic> {
        let (broker, broker_handle) = spawn_broker(scope, stdout, stderr);
        let (work_tx, work_rx) = unbounded::<crate::walker::ScheduledEntry>();
        let (ready_tx, ready_rx) = unbounded::<Result<(EntryTicket, EvalStep), Diagnostic>>();
        let sink = crate::exec::ParallelActionSink::new(broker.clone(), worker_count)?;

        let mut evaluators = Vec::new();
        for _ in 0..worker_count {
            let work_rx = work_rx.clone();
            let ready_tx = ready_tx.clone();
            let expr = plan.expr.clone();
            let eval_context = eval_context.clone();
            let follow_mode = plan.follow_mode;
            evaluators.push(scope.spawn(move || -> Result<(), Diagnostic> {
                while let Ok(item) = work_rx.recv() {
                    let ready = begin_entry_eval(&expr, &item.entry, follow_mode, &eval_context)
                        .map(|step| (item.ticket, step));
                    ready_tx.send(ready).map_err(|_| {
                        Diagnostic::new("internal error: parallel ready queue is unavailable", 1)
                    })?;
                }
                Ok(())
            }));
        }
        drop(work_rx);
        drop(ready_tx);

        let traversal_control = plan.traversal_control.clone();
        let control_context = eval_context.clone();
        let follow_mode = plan.follow_mode;
        let traversal_order = plan.traversal.order;
        let mut barriers = SubtreeBarrierTracker::default();
        let mut buffered: Vec<(EntryTicket, EvalStep)> = Vec::new();
        for event in walk_parallel(
            &plan.start_paths,
            plan.follow_mode,
            plan.traversal,
            worker_count,
            move |entry| {
                traversal_control_for_entry(
                    traversal_control.as_ref(),
                    follow_mode,
                    traversal_order,
                    entry,
                    &control_context,
                )
            },
        ) {
            match event {
                WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
                    if item.entry.depth >= plan.traversal.min_depth {
                        for barrier in &item.ticket.ancestor_barriers {
                            barriers.register_descendant(*barrier);
                        }

                        work_tx.send(item).map_err(|_| {
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

        drop(work_tx);
        for ready in ready_rx {
            let ready = ready?;
            buffered.push(ready);

            let mut made_progress = true;
            while made_progress {
                made_progress = false;
                let mut index = 0;
                while index < buffered.len() {
                    if !barriers.may_grant(&buffered[index].0) {
                        index += 1;
                        continue;
                    }

                    let (ticket, mut step) = buffered.swap_remove(index);
                    loop {
                        match step {
                            EvalStep::Complete(outcome) => {
                                had_action_failures |= outcome.status.had_action_failures();
                                for barrier in &ticket.ancestor_barriers {
                                    barriers.finish_descendant(*barrier);
                                }
                                break;
                            }
                            EvalStep::PendingAction {
                                request,
                                continuation,
                            } => {
                                let outcome = sink.execute(&request)?;
                                step = resume_entry_eval(continuation, outcome, &eval_context)?;
                            }
                        }
                    }
                    made_progress = true;
                }
            }
        }

        if !buffered.is_empty() {
            return Err(Diagnostic::new(
                "internal error: parallel grant queue did not fully drain",
                1,
            ));
        }

        for handle in evaluators {
            handle
                .join()
                .map_err(|_| Diagnostic::new("parallel evaluator thread panicked", 1))??;
        }

        had_action_failures |= sink.flush_all()?.had_action_failures();
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
    use super::{build_eval_context_with_loader, ordered_evaluator_workers};
    use crate::follow::FollowMode;
    use crate::parser::parse_command;
    use crate::planner::{
        ExecutionMode, ExecutionPlan, OutputAction, RuntimeAction, RuntimeExpr,
        RuntimeRequirements, TraversalOptions, TraversalOrder, plan_command,
    };
    use crate::runtime_policy::RuntimePolicy;
    use std::ffi::OsString;
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
            runtime_policy: RuntimePolicy::derive(1, TraversalOrder::PreOrder, true),
            traversal_control: None,
        }
    }

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(|item| (*item).into()).collect()
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

    #[test]
    fn ordered_exec_plans_fall_back_to_one_evaluator_for_commit_sensitive_actions() {
        let plan = plan_command(
            parse_command(&argv(&[".", "-exec", "false", "{}", ";"])).unwrap(),
            1,
        )
        .unwrap();

        assert_eq!(ordered_evaluator_workers(&plan), 1);
    }

    #[test]
    fn ordered_output_only_plans_keep_internal_parallelism() {
        let plan = plan_command(parse_command(&argv(&[".", "-print"])).unwrap(), 1).unwrap();

        assert_eq!(
            ordered_evaluator_workers(&plan),
            plan.runtime_policy.evaluation_workers
        );
    }
}
