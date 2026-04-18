use crate::diagnostics::Diagnostic;
use crate::parallel::broker::{BrokerMessage, spawn_broker};
use crate::parallel::worker::process_entry_output_only;
use crate::planner::ExecutionPlan;
use crate::runner::{RunSummary, build_eval_context, traversal_control_for_entry};
use crate::walker::{WalkEvent, walk_parallel};
use crossbeam_channel::{bounded, unbounded};
use std::io::Write;

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

    std::thread::scope(|scope| -> Result<(), Diagnostic> {
        let (broker, broker_handle) = spawn_broker(scope, stdout, stderr);
        let (work_tx, work_rx) = bounded::<crate::walker::ScheduledEntry>(worker_count.max(1));
        let (result_tx, result_rx) = unbounded::<Result<crate::eval::RuntimeStatus, Diagnostic>>();

        let mut workers = Vec::new();
        for _ in 0..worker_count {
            let work_rx = work_rx.clone();
            let result_tx = result_tx.clone();
            let broker = broker.clone();
            let eval_context = eval_context.clone();
            let plan = plan.clone();
            let follow_mode = plan.follow_mode;
            workers.push(scope.spawn(move || -> Result<(), Diagnostic> {
                while let Ok(item) = work_rx.recv() {
                    let status = process_entry_output_only(
                        &plan,
                        &item.entry,
                        follow_mode,
                        &eval_context,
                        &broker,
                    );
                    result_tx.send(status).map_err(|_| {
                        Diagnostic::new("internal error: v2 result queue is unavailable", 1)
                    })?;
                }
                Ok(())
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

        let mut granted = 0_usize;
        for event in walk_rx {
            match event {
                WalkEvent::Entry(item) | WalkEvent::DirectoryComplete(item) => {
                    if item.entry.depth < plan.traversal.min_depth {
                        continue;
                    }

                    work_tx.send(item).map_err(|_| {
                        Diagnostic::new("internal error: v2 worker queue is unavailable", 1)
                    })?;
                    granted += 1;
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

        let mut first_error = None;
        for _ in 0..granted {
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
                        "internal error: v2 result queue closed before all work completed",
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
