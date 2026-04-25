use crate::diagnostics::Diagnostic;
use crate::eval::{ActionSink, RuntimeStatus, evaluate_outcome_with_context};
use crate::exec::PromptCoordinator;
use crate::messages_locale::MessagesLocale;
use crate::planner::{ExecutionPlan, RuntimeExpr};
use crate::runner::{RunSummary, build_eval_context, traversal_control_for_entry};
use crate::runtime_pipeline::{EvalStep, OrderedReadyQueue, begin_entry_eval, resume_entry_eval};
use crate::walker::{OrderedWalkDirective, WalkEvent, walk_ordered};
use crossbeam_channel::unbounded;
use std::io::Write;

pub(crate) fn run_ordered_plan<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
    messages_locale: Option<MessagesLocale>,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let prompt = messages_locale
        .map(PromptCoordinator::open_process_with_locale)
        .unwrap_or_else(PromptCoordinator::open_process);
    if contains_commit_sensitive_action(&plan.expr) {
        return run_ordered_inline(plan, stdout, stderr, prompt);
    }

    run_ordered_pipeline(plan, stdout, stderr, prompt)
}

fn run_ordered_inline<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
    prompt: PromptCoordinator,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let eval_context = build_eval_context(plan)?;
    let mut sink =
        crate::exec::OrderedActionSink::with_prompt(stdout, stderr, &plan.file_outputs, prompt)?;
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
                        let outcome = evaluate_outcome_with_context(
                            &plan.expr,
                            &entry,
                            plan.follow_mode,
                            &eval_context,
                            &mut sink,
                        )?;

                        if outcome.status.is_stop_requested() {
                            return Ok(OrderedWalkDirective::Stop);
                        }
                    }
                }
                WalkEvent::Error(error) => {
                    had_runtime_errors = true;
                    sink.write_diagnostic(format!("rfd: {error}"))?;
                }
            }
            Ok(OrderedWalkDirective::Continue)
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
    prompt: PromptCoordinator,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let eval_context = build_eval_context(plan)?;
    let mut sink =
        crate::exec::OrderedActionSink::with_prompt(stdout, stderr, &plan.file_outputs, prompt)?;
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
                handle_ordered_pipeline_walk_event(
                    event,
                    plan,
                    &work_tx,
                    &mut next_sequence,
                    &mut sink,
                    &mut had_runtime_errors,
                )
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
                                &eval_context,
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

fn handle_ordered_pipeline_walk_event<W, E>(
    event: WalkEvent,
    plan: &ExecutionPlan,
    work_tx: &crossbeam_channel::Sender<(u64, crate::entry::EntryContext)>,
    next_sequence: &mut u64,
    sink: &mut crate::exec::OrderedActionSink<'_, W, E>,
    had_runtime_errors: &mut bool,
) -> Result<OrderedWalkDirective, Diagnostic>
where
    W: Write,
    E: Write,
{
    match event {
        WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
            let entry = item.entry;
            if entry.depth >= plan.traversal.min_depth {
                work_tx.send((*next_sequence, entry)).map_err(|_| {
                    Diagnostic::new(
                        "internal error: ordered evaluator channel is unavailable",
                        1,
                    )
                })?;
                *next_sequence += 1;
            }
        }
        WalkEvent::Error(error) => {
            *had_runtime_errors = true;
            sink.write_diagnostic(format!("rfd: {error}"))?;
        }
    }
    Ok(OrderedWalkDirective::Continue)
}

pub(crate) fn ordered_evaluator_workers(plan: &ExecutionPlan) -> usize {
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
        RuntimeExpr::Action(crate::planner::RuntimeAction::FilePrint { .. }) => false,
        RuntimeExpr::Action(crate::planner::RuntimeAction::FilePrintf { .. }) => false,
        RuntimeExpr::Action(_) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => false,
    }
}
