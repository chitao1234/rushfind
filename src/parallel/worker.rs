use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{
    ActionOutcome, ActionSink, EvalContext, RuntimeStatus, evaluate_outcome_with_context,
};
use crate::exec::{ImmediateExecAction, delete_path, run_immediate_parallel};
use crate::file_output::{SharedFileOutputs, render_file_print_bytes};
use crate::follow::FollowMode;
use crate::output::render_runtime_action_bytes;
use crate::parallel::batch::WorkerBatchState;
use crate::parallel::broker::BrokerMessage;
use crate::parallel::control::GlobalControl;
use crate::parallel::postorder::BarrierTable;
use crate::parallel::scheduler::{Scheduler, WorkerHandle};
use crate::parallel::task::{ParallelTask, PostOrderResumeTask, PreOrderRootTask};
use crate::planner::{ExecutionPlan, RuntimeAction, TraversalOrder};
use crate::runner::traversal_control_for_entry;
use crate::runtime_pipeline::SubtreeBarrierId;
use crate::walker::{FsWalkBackend, PendingPath, WalkBackend, should_descend_directory};
use crossbeam_channel::Sender;
use std::path::Path;
use std::sync::Arc;

const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;
const SPLIT_CHILD_THRESHOLD: usize = 32;

pub(crate) struct WorkerActionSink {
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    file_outputs: SharedFileOutputs,
    batches: WorkerBatchState,
}

pub(crate) struct WorkerReport {
    pub(crate) status: RuntimeStatus,
    pub(crate) had_runtime_errors: bool,
}

enum PostOrderFrame {
    Visit(PendingPath),
    CompleteInline {
        entry: EntryContext,
        notify_parent: Option<SubtreeBarrierId>,
    },
}

impl WorkerActionSink {
    pub(crate) fn new(
        control: Arc<GlobalControl>,
        broker: Sender<BrokerMessage>,
        file_outputs: SharedFileOutputs,
    ) -> Self {
        Self {
            control,
            broker,
            file_outputs,
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
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        match action {
            RuntimeAction::Output(_) | RuntimeAction::Printf(_) => {
                let bytes = render_runtime_action_bytes(action, entry, follow_mode, context)?;
                self.broker
                    .send(BrokerMessage::Stdout(bytes))
                    .map_err(|_| Diagnostic::new("internal error: v2 broker is unavailable", 1))?;
                Ok(ActionOutcome::matched_true())
            }
            RuntimeAction::FilePrint {
                destination,
                terminator,
            } => {
                let bytes = render_file_print_bytes(entry, *terminator);
                self.file_outputs.write_record(*destination, &bytes)?;
                Ok(ActionOutcome::matched_true())
            }
            RuntimeAction::FilePrintf {
                destination,
                program,
            } => {
                let bytes =
                    crate::printf::render_printf_bytes(program, entry, follow_mode, context)?;
                self.file_outputs.write_record(*destination, &bytes)?;
                Ok(ActionOutcome::matched_true())
            }
            RuntimeAction::Ls | RuntimeAction::FileLs { .. } => Err(Diagnostic::new(
                "internal error: ls actions are not wired into parallel execution yet",
                1,
            )),
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_parallel_worker(
    mut worker: WorkerHandle,
    scheduler: Arc<Scheduler>,
    barriers: Arc<BarrierTable>,
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    file_outputs: SharedFileOutputs,
    plan: ExecutionPlan,
    eval_context: EvalContext,
    result_tx: Sender<Result<WorkerReport, Diagnostic>>,
) -> Result<(), Diagnostic> {
    let send_result = |result| {
        result_tx
            .send(result)
            .map_err(|_| Diagnostic::new("internal error: v2 result queue is unavailable", 1))
    };
    let backend = FsWalkBackend;
    let mut sink = WorkerActionSink::new(control.clone(), broker, file_outputs);
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
            ParallelTask::PreOrderRoot(task) => {
                if plan.traversal.order == TraversalOrder::DepthFirstPostOrder {
                    run_postorder_root_task(
                        &plan,
                        &backend,
                        task,
                        &mut worker,
                        barriers.as_ref(),
                        &eval_context,
                        &mut sink,
                        &mut had_runtime_errors,
                    )
                } else {
                    run_preorder_root_serial(
                        &plan,
                        &backend,
                        task,
                        &mut worker,
                        &eval_context,
                        &mut sink,
                        &mut had_runtime_errors,
                    )
                }
            }
            ParallelTask::PostOrderResume(task) => run_postorder_resume(
                &plan,
                task,
                &mut worker,
                &eval_context,
                &mut sink,
                barriers.as_ref(),
            ),
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
    worker: &mut WorkerHandle,
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

        let mut discovered = children
            .into_iter()
            .map(|child| PendingPath {
                path: child.path,
                root_path: pending.root_path.clone(),
                depth: pending.depth + 1,
                is_command_line_root: false,
                physical_file_type_hint: child.physical_file_type_hint,
                ancestry: child_ancestry.clone(),
                ancestor_barriers: pending.ancestor_barriers.clone(),
                root_device,
                parent_completion: None,
            })
            .collect::<Vec<_>>();

        if discovered.len() >= SPLIT_CHILD_THRESHOLD && sink.control.accepts_new_work() {
            let spill = discovered.split_off(1);
            for pending_child in spill {
                worker.push_local(
                    ParallelTask::PreOrderRoot(PreOrderRootTask {
                        pending: pending_child,
                    }),
                    sink.control.as_ref(),
                );
            }
        }

        for child in discovered.into_iter().rev() {
            stack.push(child);
        }
    }

    Ok(status)
}

#[allow(clippy::too_many_arguments)]
fn run_postorder_root_task(
    plan: &ExecutionPlan,
    backend: &dyn WalkBackend,
    root: PreOrderRootTask,
    worker: &mut WorkerHandle,
    barriers: &BarrierTable,
    context: &EvalContext,
    sink: &mut WorkerActionSink,
    had_runtime_errors: &mut bool,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut status = RuntimeStatus::default();
    let root_pending = root.pending;
    let root_path = root_pending.path.clone();
    let root_depth = root_pending.depth;
    let mut root_notify_parent = root_pending.parent_completion.map(SubtreeBarrierId);
    let mut aborted_by_stop = false;
    let mut stack = vec![PostOrderFrame::Visit(root_pending)];

    while let Some(frame) = stack.pop() {
        if sink.control.quit_seen() || sink.control.fatal_error_seen() {
            aborted_by_stop = true;
            break;
        }

        match frame {
            PostOrderFrame::Visit(pending) => {
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

                let is_directory = match backend.active_directory_identity(&entry, plan.follow_mode)
                {
                    Ok(identity) => identity.is_some(),
                    Err(error) => {
                        sink.emit_runtime_error(error)?;
                        *had_runtime_errors = true;
                        continue;
                    }
                };

                if !is_directory {
                    if entry.depth >= plan.traversal.min_depth {
                        status = status.merge(process_entry_preorder_fast_path(
                            plan,
                            &entry,
                            plan.follow_mode,
                            context,
                            sink,
                        )?);
                        if status.is_stop_requested() {
                            aborted_by_stop = true;
                            break;
                        }
                    }

                    if pending.path == root_path
                        && pending.depth == root_depth
                        && let Some(parent) = root_notify_parent.take()
                    {
                        notify_parent_barrier(parent, barriers, worker, sink.control.as_ref())?;
                    }
                    continue;
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
                        if pending.path == root_path
                            && pending.depth == root_depth
                            && let Some(parent) = root_notify_parent.take()
                        {
                            notify_parent_barrier(parent, barriers, worker, sink.control.as_ref())?;
                        }
                        continue;
                    }
                };

                let Some((child_ancestry, root_device)) = descend else {
                    let notify_parent = if pending.path == root_path && pending.depth == root_depth
                    {
                        root_notify_parent.take()
                    } else {
                        None
                    };
                    stack.push(PostOrderFrame::CompleteInline {
                        entry,
                        notify_parent,
                    });
                    continue;
                };

                let (children, diagnostics) = match backend.read_children(&pending.path) {
                    Ok(result) => result,
                    Err(error) => {
                        sink.emit_runtime_error(error)?;
                        *had_runtime_errors = true;
                        let notify_parent =
                            if pending.path == root_path && pending.depth == root_depth {
                                root_notify_parent.take()
                            } else {
                                None
                            };
                        stack.push(PostOrderFrame::CompleteInline {
                            entry,
                            notify_parent,
                        });
                        continue;
                    }
                };

                for error in diagnostics {
                    sink.emit_runtime_error(error)?;
                    *had_runtime_errors = true;
                }

                let mut discovered = children
                    .into_iter()
                    .map(|child| PendingPath {
                        path: child.path,
                        root_path: pending.root_path.clone(),
                        depth: pending.depth + 1,
                        is_command_line_root: false,
                        physical_file_type_hint: child.physical_file_type_hint,
                        ancestry: child_ancestry.clone(),
                        ancestor_barriers: pending.ancestor_barriers.clone(),
                        root_device,
                        parent_completion: None,
                    })
                    .collect::<Vec<_>>();

                let mut spilled_count = 0;
                let mut barrier = None;
                if discovered.len() >= SPLIT_CHILD_THRESHOLD && sink.control.accepts_new_work() {
                    let spill = discovered.split_off(1);
                    spilled_count = spill.len();
                    if spilled_count > 0 {
                        let notify_parent =
                            if pending.path == root_path && pending.depth == root_depth {
                                root_notify_parent.take()
                            } else {
                                None
                            };
                        let created = barriers.begin_directory(
                            entry.clone(),
                            pending.ancestor_barriers.clone(),
                            spilled_count,
                            notify_parent,
                        )?;
                        barrier = Some(created);
                        for pending_child in spill {
                            worker.push_local(
                                ParallelTask::PreOrderRoot(PreOrderRootTask {
                                    pending: PendingPath {
                                        ancestor_barriers: vec![created],
                                        parent_completion: Some(created.0),
                                        ..pending_child
                                    },
                                }),
                                sink.control.as_ref(),
                            );
                        }
                    }
                }

                if spilled_count == 0 {
                    let notify_parent = if pending.path == root_path && pending.depth == root_depth
                    {
                        root_notify_parent.take()
                    } else {
                        None
                    };
                    stack.push(PostOrderFrame::CompleteInline {
                        entry,
                        notify_parent,
                    });
                }

                let child_ancestor_barriers = barrier
                    .map(|created| vec![created])
                    .unwrap_or_else(|| pending.ancestor_barriers.clone());
                for child in discovered.into_iter().rev() {
                    stack.push(PostOrderFrame::Visit(PendingPath {
                        ancestor_barriers: child_ancestor_barriers.clone(),
                        ..child
                    }));
                }
            }
            PostOrderFrame::CompleteInline {
                entry,
                notify_parent,
            } => {
                if entry.depth >= plan.traversal.min_depth {
                    status = status.merge(process_entry_preorder_fast_path(
                        plan,
                        &entry,
                        plan.follow_mode,
                        context,
                        sink,
                    )?);
                    if status.is_stop_requested() {
                        aborted_by_stop = true;
                        break;
                    }
                }

                if let Some(parent) = notify_parent {
                    notify_parent_barrier(parent, barriers, worker, sink.control.as_ref())?;
                }
            }
        }
    }

    if !aborted_by_stop && let Some(parent) = root_notify_parent.take() {
        notify_parent_barrier(parent, barriers, worker, sink.control.as_ref())?;
    }

    Ok(status)
}

fn run_postorder_resume(
    plan: &ExecutionPlan,
    task: PostOrderResumeTask,
    worker: &mut WorkerHandle,
    context: &EvalContext,
    sink: &mut WorkerActionSink,
    barriers: &BarrierTable,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut status = RuntimeStatus::default();

    if task.entry.depth >= plan.traversal.min_depth {
        status = status.merge(process_entry_preorder_fast_path(
            plan,
            &task.entry,
            plan.follow_mode,
            context,
            sink,
        )?);
    }

    if let Some(parent) = task.notify_parent {
        notify_parent_barrier(parent, barriers, worker, sink.control.as_ref())?;
    }

    Ok(status)
}

fn notify_parent_barrier(
    parent: SubtreeBarrierId,
    barriers: &BarrierTable,
    worker: &mut WorkerHandle,
    control: &GlobalControl,
) -> Result<(), Diagnostic> {
    if let Some(resume) = barriers.finish_spilled_child(parent)? {
        worker.push_local(ParallelTask::PostOrderResume(resume), control);
    }
    Ok(())
}
