use crate::diagnostics::Diagnostic;
use crate::eval::EvalContext;
use crate::output::{BrokerMessage, spawn_broker};
use crate::planner::ExecutionPlan;
use crate::runner::{RunSummary, build_eval_context, traversal_control_for_entry};
use crate::runtime_pipeline::{
    EntryTicket, EvalStep, SubtreeBarrierTracker, begin_entry_eval, resume_entry_eval,
};
use crate::walker::{WalkEvent, walk_parallel};
use crossbeam_channel::{bounded, unbounded};
use std::io::Write;

pub(crate) fn run_parallel_legacy<W, E>(
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
        let (work_tx, work_rx) = bounded::<crate::walker::ScheduledEntry>(worker_count.max(1));
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
        let walk_rx = walk_parallel(
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
        );
        let mut work_tx = Some(work_tx);
        let mut walker_closed = false;
        let mut stop_requested = false;
        let mut granted = 0_usize;
        let mut awaiting_ready = 0_usize;
        let mut barriers = SubtreeBarrierTracker::default();
        let mut buffered: Vec<(EntryTicket, EvalStep)> = Vec::new();

        let never_walk = crossbeam_channel::never::<WalkEvent>();
        let never_ready = crossbeam_channel::never::<Result<(EntryTicket, EvalStep), Diagnostic>>();

        loop {
            drain_parallel_ready_queue(
                &mut buffered,
                &mut barriers,
                &sink,
                &eval_context,
                &mut granted,
                &mut had_action_failures,
                &mut stop_requested,
            )?;

            if stop_requested {
                work_tx.take();
            }

            let mut queued_ready = false;
            while awaiting_ready > 0 {
                match ready_rx.try_recv() {
                    Ok(ready) => {
                        buffered.push(ready?);
                        awaiting_ready -= 1;
                        queued_ready = true;
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        return Err(Diagnostic::new(
                            "internal error: parallel ready queue closed before all granted work completed",
                            1,
                        ));
                    }
                }
            }
            if queued_ready {
                continue;
            }

            if walker_closed && granted == 0 {
                break;
            }

            let walk_channel = if walker_closed { &never_walk } else { &walk_rx };
            let ready_channel = if awaiting_ready == 0 {
                &never_ready
            } else {
                &ready_rx
            };

            crossbeam_channel::select! {
                recv(ready_channel) -> ready => {
                    let ready = ready.map_err(|_| {
                        Diagnostic::new(
                            "internal error: parallel ready queue closed before all granted work completed",
                            1,
                        )
                    })??;
                    buffered.push(ready);
                    awaiting_ready -= 1;
                }
                recv(walk_channel) -> event => {
                    match event {
                        Ok(WalkEvent::Entry(item)) | Ok(WalkEvent::DirectoryComplete(item)) => {
                            if stop_requested || item.entry.depth < plan.traversal.min_depth {
                                continue;
                            }

                            for barrier in &item.ticket.ancestor_barriers {
                                barriers.register_descendant(*barrier);
                            }

                            if let Some(work_tx) = &work_tx {
                                work_tx.send(item).map_err(|_| {
                                    Diagnostic::new(
                                        "internal error: parallel evaluator channel is unavailable",
                                        1,
                                    )
                                })?;
                                granted += 1;
                                awaiting_ready += 1;
                            }
                        }
                        Ok(WalkEvent::Error(error)) => {
                            if stop_requested {
                                continue;
                            }

                            had_runtime_errors = true;
                            broker
                                .send(BrokerMessage::Stderr(
                                    format!("findoxide: {error}\n").into_bytes(),
                                ))
                                .map_err(|_| {
                                    Diagnostic::new(
                                        "internal error: output broker is unavailable",
                                        1,
                                    )
                                })?;
                        }
                        Err(_) => {
                            walker_closed = true;
                            work_tx.take();
                        }
                    }
                }
            }
        }

        for ready in ready_rx {
            drop(ready?);
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

fn drain_parallel_ready_queue(
    buffered: &mut Vec<(EntryTicket, EvalStep)>,
    barriers: &mut SubtreeBarrierTracker,
    sink: &crate::exec::ParallelActionSink,
    eval_context: &EvalContext,
    granted: &mut usize,
    had_action_failures: &mut bool,
    stop_requested: &mut bool,
) -> Result<(), Diagnostic> {
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
                        *had_action_failures |= outcome.status.had_action_failures();
                        *stop_requested |= outcome.status.is_stop_requested();
                        for barrier in &ticket.ancestor_barriers {
                            barriers.finish_descendant(*barrier);
                        }
                        *granted -= 1;
                        break;
                    }
                    EvalStep::PendingAction {
                        request,
                        continuation,
                    } => {
                        let outcome = sink.execute(&request)?;
                        step = resume_entry_eval(continuation, outcome, eval_context)?;
                    }
                }
            }
            made_progress = true;
        }
    }

    Ok(())
}
