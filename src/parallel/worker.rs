use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{
    ActionOutcome, ActionSink, EvalContext, RuntimeStatus, evaluate_outcome_with_context,
};
use crate::exec::{ImmediateExecAction, delete_path, run_immediate_parallel};
use crate::follow::FollowMode;
use crate::output::render_runtime_action_bytes;
use crate::parallel::batch::WorkerBatchState;
use crate::parallel::broker::BrokerMessage;
use crate::parallel::control::GlobalControl;
use crate::parallel::scheduler::{Scheduler, WorkerHandle};
use crate::parallel::task::{ParallelTask, PostOrderResumeTask, PreOrderRootTask};
use crate::planner::{ExecutionPlan, RuntimeAction};
use crate::runner::traversal_control_for_entry;
use crate::walker::{FsWalkBackend, PendingPath, WalkBackend, should_descend_directory};
use crossbeam_channel::Sender;
use std::path::Path;
use std::sync::Arc;

const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;

pub(crate) struct WorkerActionSink {
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    batches: WorkerBatchState,
}

pub(crate) struct WorkerReport {
    pub(crate) status: RuntimeStatus,
    pub(crate) had_runtime_errors: bool,
}

impl WorkerActionSink {
    pub(crate) fn new(control: Arc<GlobalControl>, broker: Sender<BrokerMessage>) -> Self {
        Self {
            control,
            broker,
            batches: WorkerBatchState::new(DEFAULT_SPILL_THRESHOLD),
        }
    }

    pub(crate) fn flush(&mut self) -> Result<RuntimeStatus, Diagnostic> {
        self.batches.flush_all(&self.broker)
    }

    pub(crate) fn emit_runtime_error(&mut self, error: Diagnostic) -> Result<(), Diagnostic> {
        self.broker
            .send(BrokerMessage::Stderr(
                format!("findoxide: {error}\n").into_bytes(),
            ))
            .map_err(|_| Diagnostic::new("internal error: v2 broker is unavailable", 1))
    }

    fn run_immediate(
        &mut self,
        spec: &ImmediateExecAction,
        path: &Path,
    ) -> Result<ActionOutcome, Diagnostic> {
        run_immediate_parallel(spec, path, &self.broker, DEFAULT_SPILL_THRESHOLD)
            .map(ActionOutcome::new)
    }

    fn delete_now(&mut self, path: &Path) -> Result<ActionOutcome, Diagnostic> {
        match delete_path(path) {
            Ok(matched) => Ok(ActionOutcome::new(matched)),
            Err(error) => {
                self.broker
                    .send(BrokerMessage::Stderr(
                        format!("findoxide: {}\n", error.message).into_bytes(),
                    ))
                    .map_err(|_| Diagnostic::new("internal error: v2 broker is unavailable", 1))?;
                Ok(ActionOutcome {
                    matched: false,
                    status: RuntimeStatus::action_failure(),
                })
            }
        }
    }
}

impl ActionSink for WorkerActionSink {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<ActionOutcome, Diagnostic> {
        match action {
            RuntimeAction::Output(_) | RuntimeAction::Printf(_) => {
                let bytes = render_runtime_action_bytes(action, entry, follow_mode)?;
                self.broker
                    .send(BrokerMessage::Stdout(bytes))
                    .map_err(|_| Diagnostic::new("internal error: v2 broker is unavailable", 1))?;
                Ok(ActionOutcome::matched_true())
            }
            RuntimeAction::Quit => {
                self.control.request_quit();
                Ok(ActionOutcome::quit())
            }
            RuntimeAction::ExecImmediate(spec) => self.run_immediate(spec, entry.path.as_path()),
            RuntimeAction::ExecBatched(spec) => Ok(ActionOutcome {
                matched: true,
                status: self
                    .batches
                    .enqueue(spec, entry.path.as_path(), &self.broker)?,
            }),
            RuntimeAction::Delete => self.delete_now(entry.path.as_path()),
        }
    }
}

pub(crate) fn process_entry_preorder_fast_path(
    plan: &ExecutionPlan,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    sink: &mut WorkerActionSink,
) -> Result<RuntimeStatus, Diagnostic> {
    let outcome = evaluate_outcome_with_context(&plan.expr, entry, follow_mode, context, sink)?;
    Ok(outcome.status)
}

pub(crate) fn run_parallel_worker(
    mut worker: WorkerHandle,
    scheduler: Arc<Scheduler>,
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    plan: ExecutionPlan,
    eval_context: EvalContext,
    result_tx: Sender<Result<WorkerReport, Diagnostic>>,
) -> Result<(), Diagnostic> {
    let send_result = |result| {
        result_tx.send(result).map_err(|_| {
            Diagnostic::new("internal error: v2 result queue is unavailable", 1)
        })
    };
    let backend = FsWalkBackend;
    let mut sink = WorkerActionSink::new(control.clone(), broker);
    let mut status = RuntimeStatus::default();
    let mut had_runtime_errors = false;

    while let Some(task) = worker.pop_blocking(control.as_ref()) {
        if !control.accepts_new_work() {
            control.task_finished();
            if control.outstanding_tasks() == 0 {
                scheduler.notify_sleepers();
            }
            continue;
        }

        let task_result = match task {
            ParallelTask::PreOrderRoot(task) => run_preorder_root_serial(
                &plan,
                &backend,
                task,
                &scheduler,
                &eval_context,
                &mut sink,
                &mut had_runtime_errors,
            ),
            ParallelTask::PostOrderResume(task) => {
                run_postorder_resume_passthrough(task, &eval_context, &mut sink)
            }
        };

        control.task_finished();
        if control.outstanding_tasks() == 0 {
            scheduler.notify_sleepers();
        }

        match task_result {
            Ok(task_status) => {
                status = status.merge(task_status);
            }
            Err(error) => {
                control.request_fatal_stop();
                scheduler.notify_sleepers();
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
            control.request_fatal_stop();
            scheduler.notify_sleepers();
            send_result(Err(error))?;
            return Ok(());
        }
    }

    send_result(Ok(WorkerReport {
        status,
        had_runtime_errors,
    }))
}

fn run_preorder_root_serial(
    plan: &ExecutionPlan,
    backend: &dyn WalkBackend,
    root: PreOrderRootTask,
    _scheduler: &Scheduler,
    context: &EvalContext,
    sink: &mut WorkerActionSink,
    had_runtime_errors: &mut bool,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut status = RuntimeStatus::default();
    let mut stack = vec![root.pending];

    while let Some(pending) = stack.pop() {
        if sink.control.quit_seen() || sink.control.fatal_error_seen() {
            break;
        }

        let entry = match backend.load_entry(&pending) {
            Ok(entry) => entry,
            Err(error) => {
                sink.emit_runtime_error(error)?;
                *had_runtime_errors = true;
                continue;
            }
        };

        let traversal = match traversal_control_for_entry(
            plan.traversal_control.as_ref(),
            plan.follow_mode,
            plan.traversal.order,
            &entry,
            context,
        ) {
            Ok(control) => control,
            Err(error) => {
                sink.emit_runtime_error(error)?;
                *had_runtime_errors = true;
                continue;
            }
        };

        if pending.depth >= plan.traversal.min_depth {
            status = status.merge(process_entry_preorder_fast_path(
                plan,
                &entry,
                plan.follow_mode,
                context,
                sink,
            )?);
            if status.is_stop_requested() {
                break;
            }
        }

        let descend = match should_descend_directory(
            &pending,
            &entry,
            plan.follow_mode,
            plan.traversal,
            traversal,
            backend,
        ) {
            Ok(result) => result,
            Err(error) => {
                sink.emit_runtime_error(error)?;
                *had_runtime_errors = true;
                continue;
            }
        };

        let Some((child_ancestry, root_device)) = descend else {
            continue;
        };

        let (children, diagnostics) = match backend.read_children(&pending.path) {
            Ok(result) => result,
            Err(error) => {
                sink.emit_runtime_error(error)?;
                *had_runtime_errors = true;
                continue;
            }
        };

        for error in diagnostics {
            sink.emit_runtime_error(error)?;
            *had_runtime_errors = true;
        }

        for child in children.into_iter().rev() {
            stack.push(PendingPath {
                path: child.path,
                root_path: pending.root_path.clone(),
                depth: pending.depth + 1,
                is_command_line_root: false,
                physical_file_type_hint: child.physical_file_type_hint,
                ancestry: child_ancestry.clone(),
                ancestor_barriers: pending.ancestor_barriers.clone(),
                root_device,
                parent_completion: None,
            });
        }
    }

    Ok(status)
}

fn run_postorder_resume_passthrough(
    _task: PostOrderResumeTask,
    _context: &EvalContext,
    _sink: &mut WorkerActionSink,
) -> Result<RuntimeStatus, Diagnostic> {
    Ok(RuntimeStatus::default())
}
