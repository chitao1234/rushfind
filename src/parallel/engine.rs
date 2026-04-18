use crate::diagnostics::Diagnostic;
use crate::file_output::SharedFileOutputs;
use crate::parallel::broker::spawn_broker;
use crate::parallel::control::GlobalControl;
use crate::parallel::postorder::BarrierTable;
use crate::parallel::scheduler::Scheduler;
use crate::parallel::task::{ParallelTask, PreOrderRootTask};
use crate::parallel::worker::{WorkerReport, run_parallel_worker};
use crate::planner::ExecutionPlan;
use crate::runner::{RunSummary, build_eval_context};
use crossbeam_channel::unbounded;
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
    let file_outputs = SharedFileOutputs::open_all(&plan.file_outputs)?;
    let worker_count = plan.runtime_policy.requested_workers.max(1);
    let control = Arc::new(GlobalControl::new());
    let scheduler = Arc::new(Scheduler::new(worker_count));
    let barriers = Arc::new(BarrierTable::default());

    std::thread::scope(|scope| -> Result<RunSummary, Diagnostic> {
        let (broker, broker_handle) = spawn_broker(scope, stdout, stderr);
        let (result_tx, result_rx) = unbounded::<Result<WorkerReport, Diagnostic>>();

        for path in &plan.start_paths {
            scheduler.push_root(
                ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(path.clone(), 0)),
                control.as_ref(),
            );
        }

        let mut workers = Vec::new();
        for worker_index in 0..worker_count {
            let worker_handle = scheduler.worker_handle(worker_index);
            let scheduler = scheduler.clone();
            let barriers = barriers.clone();
            let control = control.clone();
            let broker = broker.clone();
            let file_outputs = file_outputs.clone();
            let plan = plan.clone();
            let eval_context = eval_context.clone();
            let result_tx = result_tx.clone();
            workers.push(scope.spawn(move || {
                run_parallel_worker(
                    worker_handle,
                    scheduler,
                    barriers,
                    control,
                    broker,
                    file_outputs,
                    plan,
                    eval_context,
                    result_tx,
                )
            }));
        }
        drop(result_tx);

        let mut first_error = None;
        let mut had_runtime_errors = false;
        let mut had_action_failures = false;
        for _ in 0..worker_count {
            match result_rx.recv() {
                Ok(Ok(report)) => {
                    had_runtime_errors |= report.had_runtime_errors;
                    had_action_failures |= report.status.had_action_failures();
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

        Ok(RunSummary {
            had_runtime_errors,
            had_action_failures,
        })
    })
}
