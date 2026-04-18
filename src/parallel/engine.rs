use crate::diagnostics::Diagnostic;
use crate::parallel::broker::{BrokerMessage, spawn_broker};
use crate::parallel::control::GlobalControl;
use crate::parallel::worker::{WorkerActionSink, process_entry_preorder_fast_path};
use crate::planner::ExecutionPlan;
use crate::runner::{RunSummary, build_eval_context, traversal_control_for_entry};
use crate::walker::{WalkEvent, walk_parallel};
use crossbeam_channel::{bounded, unbounded};
use std::io::Write;
use std::sync::Arc;

pub(crate) fn run_parallel_v2<W, E>(
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
    let control = Arc::new(GlobalControl::new());

    std::thread::scope(|scope| -> Result<(), Diagnostic> {
        let (broker, broker_handle) = spawn_broker(scope, stdout, stderr);
        let (work_tx, work_rx) = bounded::<crate::walker::ScheduledEntry>(worker_count.max(1));
        let (result_tx, result_rx) = unbounded::<Result<crate::eval::RuntimeStatus, Diagnostic>>();

        let mut workers = Vec::new();
        for _ in 0..worker_count {
            let work_rx = work_rx.clone();
            let result_tx = result_tx.clone();
            let broker = broker.clone();
            let control = control.clone();
            let eval_context = eval_context.clone();
            let plan = plan.clone();
            let follow_mode = plan.follow_mode;
            workers.push(scope.spawn(move || -> Result<(), Diagnostic> {
                let send_result = |result| {
                    result_tx.send(result).map_err(|_| {
                        Diagnostic::new("internal error: v2 result queue is unavailable", 1)
                    })
                };
                let mut sink = WorkerActionSink::new(control, broker);
                let mut status = crate::eval::RuntimeStatus::default();

                while let Ok(item) = work_rx.recv() {
                    match process_entry_preorder_fast_path(
                        &plan,
                        &item.entry,
                        follow_mode,
                        &eval_context,
                        &mut sink,
                    ) {
                        Ok(entry_status) => {
                            status = status.merge(entry_status);
                        }
                        Err(error) => {
                            send_result(Err(error))?;
                            return Ok(());
                        }
                    }
                }

                match sink.flush() {
                    Ok(flush_status) => {
                        status = status.merge(flush_status);
                    }
                    Err(error) => {
                        send_result(Err(error))?;
                        return Ok(());
                    }
                }

                send_result(Ok(status))
            }));
        }
        drop(work_rx);
        drop(result_tx);

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

        for event in walk_rx {
            match event {
                WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
                    if !control.accepts_new_work() || item.entry.depth < plan.traversal.min_depth {
                        continue;
                    }

                    work_tx.send(item).map_err(|_| {
                        Diagnostic::new("internal error: v2 worker queue is unavailable", 1)
                    })?;
                }
                WalkEvent::Error(error) => {
                    if control.quit_seen() {
                        continue;
                    }

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

        let mut first_error = None;
        for _ in 0..worker_count {
            match result_rx.recv() {
                Ok(Ok(status)) => {
                    had_action_failures |= status.had_action_failures();
                }
                Ok(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    return Err(Diagnostic::new(
                        "internal error: v2 result queue closed before all workers completed",
                        1,
                    ));
                }
            }
        }

        for handle in workers {
            handle
                .join()
                .map_err(|_| Diagnostic::new("v2 worker thread panicked", 1))??;
        }

        drop(broker);
        broker_handle
            .join()
            .map_err(|_| Diagnostic::new("output broker thread panicked", 1))??;

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    })?;

    Ok(RunSummary {
        had_runtime_errors,
        had_action_failures,
    })
}
