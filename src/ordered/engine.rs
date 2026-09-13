use crate::diagnostics::Diagnostic;
use crate::eval::{ActionSink, EvalContext, RuntimeStatus};
use crate::exec::OrderedActionSink;
use crate::exec::PromptCoordinator;
use crate::follow::FollowMode;
use crate::messages_locale::MessagesLocale;
use crate::planner::{ExecutionPlan, RuntimeExpr};
use crate::runner::{RunSummary, build_eval_context, traversal_control_for_entry};
use crate::runtime_pipeline::{
    EvalStep, OrderedItem, OrderedPipelineControl, OrderedReady, OrderedReadyQueue,
    begin_entry_eval, resume_entry_eval,
};
use crate::walker::{OrderedWalkDirective, WalkEvent, walk_ordered};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::io::Write;
use std::sync::Arc;

/// Entries in flight per worker. The window bounds how far the walker may run
/// ahead of the collector, and therefore the pipeline's memory footprint.
const ENTRIES_PER_WORKER: usize = 4;
const MIN_WINDOW: usize = 32;
const MAX_WINDOW: usize = 256;

pub(crate) fn run_ordered_plan<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
    messages_locale: Option<MessagesLocale>,
) -> Result<RunSummary, Diagnostic>
where
    W: Write + Send,
    E: Write + Send,
{
    let prompt = messages_locale
        .map(PromptCoordinator::open_process_with_locale)
        .unwrap_or_else(PromptCoordinator::open_process);
    let eval_context = build_eval_context(plan)?;

    if ordered_evaluator_workers(plan) <= 1 {
        run_ordered_inline(plan, stdout, stderr, prompt, &eval_context)
    } else {
        run_ordered_streaming(plan, stdout, stderr, prompt, &eval_context)
    }
}

/// Single-threaded ordered execution: the walk, evaluation, and dispatch all
/// happen on the calling thread, so nothing is buffered and output starts with
/// the first entry.
fn run_ordered_inline<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
    prompt: PromptCoordinator,
    eval_context: &EvalContext,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let mut sink =
        crate::exec::OrderedActionSink::with_prompt(stdout, stderr, &plan.file_outputs, prompt)?;
    let mut had_runtime_errors = false;
    let mut requested_exit = None;

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
                eval_context,
            )
        },
        |event| {
            match event {
                WalkEvent::Entry(entry) | WalkEvent::DirectoryComplete(entry) => {
                    if entry.depth >= plan.traversal.min_depth {
                        let outcome = crate::eval::evaluate_outcome_with_context(
                            &plan.expr,
                            &entry,
                            plan.follow_mode,
                            eval_context,
                            &mut sink,
                        )?;

                        if outcome.status.is_stop_requested() {
                            requested_exit = outcome.status.requested_exit();
                            return Ok(OrderedWalkDirective::Stop);
                        }
                    }
                }
                WalkEvent::Error(error) => {
                    had_runtime_errors = true;
                    sink.flush_stdout()?;
                    sink.write_diagnostic(error)?;
                }
            }
            Ok(OrderedWalkDirective::Continue)
        },
    )?;

    let had_action_failures = sink.flush()?.had_action_failures();

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
        requested_exit,
    })
}

/// Parallel ordered execution: the walker publishes sequence-numbered items
/// into a bounded channel, evaluators process them, and a collector releases
/// them in order. Both channels are bounded, so in-flight memory is a function
/// of the worker count rather than of the tree size.
fn run_ordered_streaming<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
    prompt: PromptCoordinator,
    eval_context: &EvalContext,
) -> Result<RunSummary, Diagnostic>
where
    W: Write + Send,
    E: Write + Send,
{
    let workers = ordered_evaluator_workers(plan);
    let window = pipeline_window(workers);
    let control = Arc::new(OrderedPipelineControl::default());

    std::thread::scope(|scope| -> Result<RunSummary, Diagnostic> {
        let (work_tx, work_rx) = bounded::<(u64, OrderedItem)>(window);
        let (ready_tx, ready_rx) = bounded::<(u64, OrderedReady)>(window);
        // One permit per sequence that may be in flight. The walker takes a
        // permit before publishing and the collector returns one when it
        // dispatches, so the number of published-but-undispatched entries can
        // never exceed the window - which is what lets the release ring be
        // sized to hold all of them.
        let (permit_tx, permit_rx) = bounded::<()>(window);
        for _ in 0..window {
            permit_tx
                .send(())
                .map_err(|_| Diagnostic::new("internal error: ordered permits", 1))?;
        }

        let mut evaluators = Vec::with_capacity(workers);
        for _ in 0..workers {
            let work_rx = work_rx.clone();
            let ready_tx = ready_tx.clone();
            let control = control.clone();
            let expr = plan.expr.clone();
            let follow_mode = plan.follow_mode;
            let eval_context = eval_context.clone();
            evaluators.push(scope.spawn(move || {
                run_ordered_evaluator(work_rx, ready_tx, expr, follow_mode, eval_context, control)
            }));
        }
        drop(work_rx);
        drop(ready_tx);

        let collector = {
            let control = control.clone();
            scope.spawn(move || {
                run_ordered_collector(
                    plan,
                    ready_rx,
                    permit_tx,
                    stdout,
                    stderr,
                    prompt,
                    eval_context,
                    control,
                    window,
                )
            })
        };

        let mut next_sequence = 0_u64;
        let walk_result = walk_ordered(
            &plan.start_paths,
            plan.follow_mode,
            plan.traversal,
            |entry| {
                traversal_control_for_entry(
                    plan.traversal_control.as_ref(),
                    plan.follow_mode,
                    plan.traversal.order,
                    entry,
                    eval_context,
                )
            },
            |event| {
                publish_ordered_event(event, plan, &work_tx, &permit_rx, &control, &mut next_sequence)
            },
        );
        drop(work_tx);

        for handle in evaluators {
            handle
                .join()
                .map_err(|_| Diagnostic::new("ordered evaluator thread panicked", 1))?;
        }

        let summary = collector
            .join()
            .map_err(|_| Diagnostic::new("ordered collector thread panicked", 1))?;
        // The collector observes the reason the pipeline wound down; the walk
        // only sees the pipeline disappear underneath it, so its error is the
        // less informative of the two.
        let summary = summary?;
        walk_result?;
        Ok(summary)
    })
}

fn publish_ordered_event(
    event: WalkEvent,
    plan: &ExecutionPlan,
    work_tx: &Sender<(u64, OrderedItem)>,
    permit_rx: &Receiver<()>,
    control: &OrderedPipelineControl,
    next_sequence: &mut u64,
) -> Result<OrderedWalkDirective, Diagnostic> {
    if control.is_stopped() {
        return Ok(OrderedWalkDirective::Stop);
    }

    let item = match event {
        WalkEvent::Entry(entry) | WalkEvent::DirectoryComplete(entry) => {
            if entry.depth < plan.traversal.min_depth {
                return Ok(OrderedWalkDirective::Continue);
            }
            OrderedItem::Entry(entry)
        }
        WalkEvent::Error(error) => OrderedItem::Diagnostic(error),
    };

    // Blocks once the window is full, which is the pipeline's only backpressure.
    if permit_rx.recv().is_err() {
        return Ok(OrderedWalkDirective::Stop);
    }

    let sequence = *next_sequence;
    *next_sequence += 1;
    if work_tx.send((sequence, item)).is_err() {
        return Ok(OrderedWalkDirective::Stop);
    }

    Ok(OrderedWalkDirective::Continue)
}

fn run_ordered_evaluator(
    work_rx: Receiver<(u64, OrderedItem)>,
    ready_tx: Sender<(u64, OrderedReady)>,
    expr: RuntimeExpr,
    follow_mode: FollowMode,
    eval_context: EvalContext,
    control: Arc<OrderedPipelineControl>,
) {
    while let Ok((sequence, item)) = work_rx.recv() {
        let ready = match item {
            OrderedItem::Diagnostic(error) => OrderedReady::Diagnostic(error),
            OrderedItem::Entry(entry) => {
                if control.is_stopped() {
                    // The run is winding down; the collector discards anything
                    // that arrives after the stop, so do not evaluate it.
                    continue;
                }

                match begin_entry_eval(&expr, &entry, follow_mode, &eval_context) {
                    Ok(step) => OrderedReady::Step(step),
                    Err(error) => OrderedReady::Fatal(error),
                }
            }
        };

        if ready_tx.send((sequence, ready)).is_err() {
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_ordered_collector<W, E>(
    plan: &ExecutionPlan,
    ready_rx: Receiver<(u64, OrderedReady)>,
    permit_tx: Sender<()>,
    stdout: &mut W,
    stderr: &mut E,
    prompt: PromptCoordinator,
    eval_context: &EvalContext,
    control: Arc<OrderedPipelineControl>,
    window: usize,
) -> Result<RunSummary, Diagnostic>
where
    W: Write,
    E: Write,
{
    let mut sink =
        OrderedActionSink::with_prompt(stdout, stderr, &plan.file_outputs, prompt)?;
    // The window bounds how many entries can be published but not dispatched,
    // so the ring only has to hold that many.
    let mut queue = OrderedReadyQueue::with_capacity(window);
    let mut status = RuntimeStatus::default();
    let mut had_runtime_errors = false;
    let mut requested_exit = None;
    let mut stopped = false;

    for (sequence, ready) in ready_rx {
        // After a stop the remaining results are drained without being
        // dispatched, which is what lets the evaluators finish and exit.
        if stopped {
            continue;
        }

        match ready {
            OrderedReady::Fatal(error) => {
                control.request_stop();
                return Err(error);
            }
            OrderedReady::Step(step) => queue.insert(sequence, CollectorItem::Step(step))?,
            OrderedReady::Diagnostic(error) => {
                queue.insert(sequence, CollectorItem::Diagnostic(error))?
            }
        }

        while let Some(item) = queue.pop_next() {
            // Releasing an item frees its window slot for the walker.
            let _ = permit_tx.try_send(());

            match item {
                CollectorItem::Diagnostic(error) => {
                    had_runtime_errors = true;
                    // Keep merged stdout/stderr in traversal order.
                    sink.flush_stdout()?;
                    sink.write_diagnostic(error)?;
                }
                CollectorItem::Step(step) => {
                    let outcome = dispatch_ordered_step(step, &mut sink, eval_context)?;
                    status = status.merge(outcome.status);
                    requested_exit = requested_exit.or(outcome.status.requested_exit());
                    if outcome.status.is_stop_requested() {
                        stopped = true;
                        control.request_stop();
                        // A walker blocked on a window slot has to be let
                        // through: it re-checks the stop flag before it
                        // publishes anything else.
                        release_window_slots(&permit_tx);
                        // Results already released for later entries belong
                        // after the entry that asked to stop, so they must not
                        // be dispatched even though they are in order.
                        break;
                    }
                }
            }
        }
    }

    sink.flush_stdout()?;
    status = status.merge(sink.flush()?);

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures: status.had_action_failures(),
        requested_exit,
    })
}

enum CollectorItem {
    Step(EvalStep),
    Diagnostic(Diagnostic),
}

/// Runs an entry's expression to completion, executing each pending action in
/// sequence order and feeding the action outcome back into the continuation.
fn dispatch_ordered_step<W, E>(
    mut step: EvalStep,
    sink: &mut OrderedActionSink<'_, W, E>,
    eval_context: &EvalContext,
) -> Result<crate::eval::EvalOutcome, Diagnostic>
where
    W: Write,
    E: Write,
{
    loop {
        match step {
            EvalStep::Complete(outcome) => return Ok(outcome),
            EvalStep::PendingAction {
                request,
                continuation,
            } => {
                let outcome = sink.dispatch(
                    request.action(),
                    request.entry(),
                    request.follow_mode(),
                    eval_context,
                )?;
                step = resume_entry_eval(continuation, outcome, eval_context)?;
            }
        }
    }
}

pub(crate) fn ordered_evaluator_workers(plan: &ExecutionPlan) -> usize {
    plan.runtime_policy.evaluation_workers.max(1)
}

fn pipeline_window(workers: usize) -> usize {
    (workers * ENTRIES_PER_WORKER).clamp(MIN_WINDOW, MAX_WINDOW)
}

/// Hand every free window slot back, so a walker that is blocked waiting for
/// one can observe the stop request and unwind.
fn release_window_slots(permit_tx: &Sender<()>) {
    while permit_tx.try_send(()).is_ok() {}
}
