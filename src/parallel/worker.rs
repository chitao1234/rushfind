use crate::action_output::{RenderedAction, render_action_output};
use crate::diagnostics::{Diagnostic, internal_unavailable, runtime_stderr_line};
use crate::entry::EntryContext;
use crate::eval::{
    ActionOutcome, ActionSink, EvalContext, RuntimeStatus, evaluate_outcome_with_context,
};
use crate::exec::{
    ConfirmOutcome, ImmediateExecAction, PromptCoordinator, build_immediate_command, delete_path,
    render_prompt_argv, run_immediate_parallel, run_prepared_inherited,
};
use crate::file_output::SharedFileOutputs;
use crate::follow::FollowMode;
use crate::parallel::batch::WorkerBatchState;
use crate::parallel::broker::BrokerMessage;
use crate::parallel::chunking::{
    ChunkAccumulator, ChunkPlan, DEFAULT_SPILL_CHUNK_SIZE, DEFAULT_SPLIT_CHILD_THRESHOLD,
};
use crate::parallel::control::GlobalControl;
use crate::parallel::postorder::{BarrierRelease, BarrierTable};
use crate::parallel::scheduler::{Scheduler, WorkerHandle};
use crate::parallel::task::{
    ParallelTask, PostOrderResumeTask, PreOrderRootTask, SiblingChunkTask,
};
use crate::planner::{ExecutionPlan, RuntimeAction, TraversalOrder};
use crate::runner::traversal_control_for_entry;
use crate::runtime_pipeline::SubtreeBarrierId;
use crate::walker::{
    DiscoveredChild, FsWalkBackend, PendingPath, WalkBackend, should_descend_directory,
};
use crossbeam_channel::Sender;
use std::path::Path;
use std::sync::Arc;

mod context;
use context::{PostorderRunContext, WorkerRunContext};

const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;
pub(crate) struct WorkerActionSink {
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    file_outputs: SharedFileOutputs,
    batches: WorkerBatchState,
    prompt: Arc<PromptCoordinator>,
}

pub(crate) struct WorkerReport {
    pub(crate) status: RuntimeStatus,
    pub(crate) had_runtime_errors: bool,
}

impl WorkerActionSink {
    pub(crate) fn new(
        control: Arc<GlobalControl>,
        broker: Sender<BrokerMessage>,
        file_outputs: SharedFileOutputs,
        prompt: Arc<PromptCoordinator>,
    ) -> Self {
        Self {
            control,
            broker,
            file_outputs,
            batches: WorkerBatchState::new(DEFAULT_SPILL_THRESHOLD),
            prompt,
        }
    }

    pub(crate) fn flush(&mut self) -> Result<RuntimeStatus, Diagnostic> {
        self.batches.flush_all(&self.broker)
    }

    pub(crate) fn emit_runtime_error(&mut self, error: Diagnostic) -> Result<(), Diagnostic> {
        self.broker
            .send(BrokerMessage::Stderr(runtime_stderr_line(error)))
            .map_err(|_| internal_unavailable("v2 broker"))
    }

    fn run_immediate(
        &mut self,
        spec: &ImmediateExecAction,
        path: &Path,
    ) -> Result<ActionOutcome, Diagnostic> {
        run_immediate_parallel(spec, path, &self.broker, DEFAULT_SPILL_THRESHOLD)
            .map(ActionOutcome::new)
    }

    fn run_prompted(
        &mut self,
        spec: &ImmediateExecAction,
        path: &Path,
    ) -> Result<ActionOutcome, Diagnostic> {
        let prompt_argv = render_prompt_argv(spec, path);
        let prepared = build_immediate_command(spec, path);
        match self
            .prompt
            .confirm_prepared(&prompt_argv, &prepared, |prepared| {
                let mut stderr = std::io::stderr();
                run_prepared_inherited(prepared, &mut stderr)
            }) {
            Ok(ConfirmOutcome::Accepted(true)) => Ok(ActionOutcome::matched_true()),
            Ok(ConfirmOutcome::Accepted(false)) => Ok(ActionOutcome {
                matched: false,
                status: RuntimeStatus::action_failure(),
            }),
            Ok(ConfirmOutcome::Rejected) => Ok(ActionOutcome::new(false)),
            Err(error) => {
                self.emit_runtime_error(error)?;
                Ok(ActionOutcome {
                    matched: false,
                    status: RuntimeStatus::action_failure(),
                })
            }
        }
    }

    fn delete_now(&mut self, path: &Path) -> Result<ActionOutcome, Diagnostic> {
        match delete_path(path) {
            Ok(matched) => Ok(ActionOutcome::new(matched)),
            Err(error) => {
                self.broker
                    .send(BrokerMessage::Stderr(runtime_stderr_line(&error.message)))
                    .map_err(|_| internal_unavailable("v2 broker"))?;
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
        if let Some(rendered) = render_action_output(action, entry, follow_mode, context)? {
            match rendered {
                RenderedAction::Stdout(bytes) => {
                    self.broker
                        .send(BrokerMessage::Stdout(bytes))
                        .map_err(|_| internal_unavailable("v2 broker"))?;
                }
                RenderedAction::File { destination, bytes } => {
                    self.file_outputs.write_record(destination, &bytes)?;
                }
            }
            return Ok(ActionOutcome::matched_true());
        }

        match action {
            RuntimeAction::Quit => {
                self.control.request_quit();
                Ok(ActionOutcome::quit())
            }
            RuntimeAction::ExecImmediate(spec) => self.run_immediate(spec, entry.path.as_path()),
            RuntimeAction::ExecPrompt(spec) => self.run_prompted(spec, entry.path.as_path()),
            RuntimeAction::ExecBatched(spec) => Ok(ActionOutcome {
                matched: true,
                status: self
                    .batches
                    .enqueue(spec, entry.path.as_path(), &self.broker)?,
            }),
            RuntimeAction::Delete => self.delete_now(entry.path.as_path()),
            RuntimeAction::Output(_)
            | RuntimeAction::Printf(_)
            | RuntimeAction::FilePrint { .. }
            | RuntimeAction::FilePrintf { .. }
            | RuntimeAction::Ls
            | RuntimeAction::FileLs { .. } => {
                unreachable!("rendered runtime action must have been handled already")
            }
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
    prompt: Arc<PromptCoordinator>,
    plan: ExecutionPlan,
    eval_context: EvalContext,
    result_tx: Sender<Result<WorkerReport, Diagnostic>>,
) -> Result<(), Diagnostic> {
    let send_result = |result| {
        result_tx
            .send(result)
            .map_err(|_| internal_unavailable("v2 result queue"))
    };
    let backend = FsWalkBackend;
    let mut sink = WorkerActionSink::new(control.clone(), broker, file_outputs, prompt);
    let mut status = RuntimeStatus::default();
    let mut had_runtime_errors = false;
    let run_context = WorkerRunContext {
        plan: &plan,
        backend: &backend,
        barriers: barriers.as_ref(),
        eval_context: &eval_context,
    };

    while let Some(task) = worker.pop_blocking(control.as_ref()) {
        if !control.accepts_new_work() {
            control.task_finished();
            if control.outstanding_tasks() == 0 {
                scheduler.notify_sleepers();
            }
            continue;
        }

        let task_result = run_parallel_task(
            task,
            run_context,
            &mut worker,
            &mut sink,
            &mut had_runtime_errors,
        );

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

fn run_parallel_task(
    task: ParallelTask,
    run: WorkerRunContext<'_>,
    worker: &mut WorkerHandle,
    sink: &mut WorkerActionSink,
    had_runtime_errors: &mut bool,
) -> Result<RuntimeStatus, Diagnostic> {
    match task {
        ParallelTask::PreOrderRoot(task) => {
            if run.plan.traversal.order == TraversalOrder::DepthFirstPostOrder {
                let mut context = PostorderRunContext {
                    run,
                    worker,
                    sink,
                    had_runtime_errors,
                };
                run_postorder_root_task(task, &mut context)
            } else {
                run_preorder_root_serial(
                    run.plan,
                    run.backend,
                    task,
                    worker,
                    run.eval_context,
                    sink,
                    had_runtime_errors,
                )
            }
        }
        ParallelTask::SiblingChunk(task) => {
            if task.completion_barrier.is_some() {
                let mut context = PostorderRunContext {
                    run,
                    worker,
                    sink,
                    had_runtime_errors,
                };
                run_postorder_pending_batch(task.pending, task.completion_barrier, &mut context)
            } else {
                run_preorder_pending_batch(
                    run.plan,
                    run.backend,
                    task.pending,
                    worker,
                    run.eval_context,
                    sink,
                    had_runtime_errors,
                )
            }
        }
        ParallelTask::PostOrderResume(task) => {
            let mut context = PostorderRunContext {
                run,
                worker,
                sink,
                had_runtime_errors,
            };
            run_postorder_resume(task, &mut context)
        }
    }
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
    run_preorder_pending_batch(
        plan,
        backend,
        vec![root.pending],
        worker,
        context,
        sink,
        had_runtime_errors,
    )
}

fn publish_preorder_sibling_chunks(
    chunks: Vec<Vec<PendingPath>>,
    worker: &mut WorkerHandle,
    control: &GlobalControl,
) {
    for pending in chunks {
        worker.push_local(
            ParallelTask::SiblingChunk(SiblingChunkTask {
                pending,
                completion_barrier: None,
            }),
            control,
        );
    }
}

fn discovered_child_to_pending(
    child: DiscoveredChild,
    pending: &PendingPath,
    child_ancestry: &[crate::identity::FileIdentity],
    root_device: Option<u64>,
) -> PendingPath {
    PendingPath {
        path: child.path,
        root_path: pending.root_path.clone(),
        depth: pending.depth + 1,
        is_command_line_root: false,
        physical_file_type_hint: child.physical_file_type_hint,
        ancestry: child_ancestry.to_vec(),
        ancestor_barriers: pending.ancestor_barriers.clone(),
        root_device,
        parent_completion: None,
    }
}

fn run_preorder_pending_batch(
    plan: &ExecutionPlan,
    backend: &dyn WalkBackend,
    initial: Vec<PendingPath>,
    worker: &mut WorkerHandle,
    context: &EvalContext,
    sink: &mut WorkerActionSink,
    had_runtime_errors: &mut bool,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut status = RuntimeStatus::default();
    let mut stack = initial.into_iter().rev().collect::<Vec<_>>();

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

        let mut chunks =
            ChunkAccumulator::new(DEFAULT_SPLIT_CHILD_THRESHOLD, DEFAULT_SPILL_CHUNK_SIZE);
        let mut emitted_error = None;
        backend.visit_children(&pending.path, &mut |item| match item {
            Ok(child) => {
                chunks.push(
                    discovered_child_to_pending(child, &pending, &child_ancestry, root_device),
                    sink.control.accepts_new_work(),
                );
                if sink.control.accepts_new_work() {
                    publish_preorder_sibling_chunks(
                        chunks.take_spilled_chunks(),
                        worker,
                        sink.control.as_ref(),
                    );
                }
            }
            Err(error) => {
                if let Err(emit_error) = sink.emit_runtime_error(error) {
                    emitted_error = Some(emit_error);
                }
                *had_runtime_errors = true;
            }
        })?;

        if let Some(error) = emitted_error {
            return Err(error);
        }

        if sink.control.quit_seen() {
            chunks.observe_quit();
        }

        let chunk_plan = chunks.finish();
        publish_preorder_sibling_chunks(chunk_plan.spilled_chunks, worker, sink.control.as_ref());
        for child in chunk_plan.local_stack.into_iter().rev() {
            stack.push(child);
        }
    }

    Ok(status)
}

fn run_postorder_root_task(
    root: PreOrderRootTask,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    run_postorder_pending_batch(vec![root.pending], None, context)
}

fn run_postorder_pending_batch(
    initial: Vec<PendingPath>,
    completion_barrier: Option<SubtreeBarrierId>,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut status = RuntimeStatus::default();
    let batch_barrier = match completion_barrier {
        Some(parent) if initial.len() > 1 => {
            Some(context.run.barriers.begin_batch(initial.len(), parent)?)
        }
        _ => None,
    };
    let root_completion = batch_barrier.or(completion_barrier);

    for mut pending in initial {
        if context.sink.control.quit_seen() || context.sink.control.fatal_error_seen() {
            break;
        }

        if let Some(parent) = root_completion {
            pending.parent_completion = Some(parent.0);
        }

        status = status.merge(run_postorder_pending_root(pending, context)?);
        if status.is_stop_requested() {
            break;
        }
    }

    Ok(status)
}

fn run_postorder_pending_root(
    pending: PendingPath,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    let run = context.run;
    let plan = run.plan;
    let notify_parent = pending.parent_completion.map(SubtreeBarrierId);
    let entry = match run.backend.load_entry(&pending) {
        Ok(entry) => entry,
        Err(error) => {
            return emit_postorder_runtime_error(error, notify_parent, context);
        }
    };

    let traversal = match traversal_control_for_entry(
        plan.traversal_control.as_ref(),
        plan.follow_mode,
        plan.traversal.order,
        &entry,
        run.eval_context,
    ) {
        Ok(control) => control,
        Err(error) => {
            return emit_postorder_runtime_error(error, notify_parent, context);
        }
    };

    let is_directory = match run
        .backend
        .active_directory_identity(&entry, plan.follow_mode)
    {
        Ok(identity) => identity.is_some(),
        Err(error) => {
            return emit_postorder_runtime_error(error, notify_parent, context);
        }
    };

    if !is_directory {
        return complete_postorder_entry(entry, notify_parent, context);
    }

    let descend = match should_descend_directory(
        &pending,
        &entry,
        plan.follow_mode,
        plan.traversal,
        traversal,
        run.backend,
    ) {
        Ok(result) => result,
        Err(error) => {
            return emit_postorder_runtime_error(error, notify_parent, context);
        }
    };

    let Some((child_ancestry, root_device)) = descend else {
        return complete_postorder_entry(entry, notify_parent, context);
    };

    let Some(chunk_plan) =
        collect_postorder_child_chunks(&pending, &child_ancestry, root_device, context)?
    else {
        return complete_postorder_entry(entry, notify_parent, context);
    };

    if context.sink.control.quit_seen() || context.sink.control.fatal_error_seen() {
        return Ok(RuntimeStatus::default());
    }

    run_postorder_child_plan(entry, notify_parent, chunk_plan, context)
}

fn collect_postorder_child_chunks(
    pending: &PendingPath,
    child_ancestry: &[crate::identity::FileIdentity],
    root_device: Option<u64>,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<Option<ChunkPlan>, Diagnostic> {
    let mut chunks = ChunkAccumulator::new(DEFAULT_SPLIT_CHILD_THRESHOLD, DEFAULT_SPILL_CHUNK_SIZE);
    let mut emitted_error = None;

    match context
        .run
        .backend
        .visit_children(&pending.path, &mut |item| match item {
            Ok(child) => chunks.push(
                discovered_child_to_pending(child, pending, child_ancestry, root_device),
                context.sink.control.accepts_new_work(),
            ),
            Err(error) => {
                if let Err(emit_error) = context.sink.emit_runtime_error(error) {
                    emitted_error = Some(emit_error);
                }
                *context.had_runtime_errors = true;
            }
        }) {
        Ok(()) => {}
        Err(error) => {
            context.sink.emit_runtime_error(error)?;
            *context.had_runtime_errors = true;
            return Ok(None);
        }
    }

    if let Some(error) = emitted_error {
        return Err(error);
    }

    Ok(Some(chunks.finish()))
}

fn run_postorder_child_plan(
    entry: EntryContext,
    notify_parent: Option<SubtreeBarrierId>,
    chunk_plan: ChunkPlan,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    let has_local_batch = !chunk_plan.local_stack.is_empty();
    let child_units = chunk_plan.spilled_chunks.len() + usize::from(has_local_batch);
    if child_units == 0 {
        return complete_postorder_entry(entry, notify_parent, context);
    }

    let barrier = context
        .run
        .barriers
        .begin_directory(entry, child_units, notify_parent)?;
    let mut status = RuntimeStatus::default();

    if has_local_batch {
        let local_batch = chunk_plan
            .local_stack
            .into_iter()
            .map(|child| PendingPath {
                ancestor_barriers: vec![barrier],
                ..child
            })
            .collect();
        status = status.merge(run_postorder_pending_batch(
            local_batch,
            Some(barrier),
            context,
        )?);
    }

    if !context.sink.control.accepts_new_work() || status.is_stop_requested() {
        return Ok(status);
    }

    for chunk in chunk_plan.spilled_chunks {
        let pending = chunk
            .into_iter()
            .map(|child| PendingPath {
                ancestor_barriers: vec![barrier],
                ..child
            })
            .collect::<Vec<_>>();
        context.worker.push_local(
            ParallelTask::SiblingChunk(SiblingChunkTask {
                pending,
                completion_barrier: Some(barrier),
            }),
            context.sink.control.as_ref(),
        );
    }

    Ok(status)
}

fn complete_postorder_entry(
    entry: EntryContext,
    notify_parent: Option<SubtreeBarrierId>,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    let plan = context.run.plan;
    let mut status = RuntimeStatus::default();

    if entry.depth >= plan.traversal.min_depth {
        status = status.merge(process_entry_preorder_fast_path(
            plan,
            &entry,
            plan.follow_mode,
            context.run.eval_context,
            context.sink,
        )?);
        if status.is_stop_requested() {
            return Ok(status);
        }
    }

    if let Some(parent) = notify_parent {
        notify_parent_barrier(
            parent,
            context.run.barriers,
            context.worker,
            context.sink.control.as_ref(),
        )?;
    }

    Ok(status)
}

fn run_postorder_resume(
    task: PostOrderResumeTask,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    let plan = context.run.plan;
    let mut status = RuntimeStatus::default();

    if task.entry.depth >= plan.traversal.min_depth {
        status = status.merge(process_entry_preorder_fast_path(
            plan,
            &task.entry,
            plan.follow_mode,
            context.run.eval_context,
            context.sink,
        )?);
    }

    if let Some(parent) = task.notify_parent {
        notify_parent_barrier(
            parent,
            context.run.barriers,
            context.worker,
            context.sink.control.as_ref(),
        )?;
    }

    Ok(status)
}

fn emit_postorder_runtime_error(
    error: Diagnostic,
    notify_parent: Option<SubtreeBarrierId>,
    context: &mut PostorderRunContext<'_, '_>,
) -> Result<RuntimeStatus, Diagnostic> {
    context.sink.emit_runtime_error(error)?;
    *context.had_runtime_errors = true;
    if let Some(parent) = notify_parent {
        notify_parent_barrier(
            parent,
            context.run.barriers,
            context.worker,
            context.sink.control.as_ref(),
        )?;
    }
    Ok(RuntimeStatus::default())
}

fn notify_parent_barrier(
    parent: SubtreeBarrierId,
    barriers: &BarrierTable,
    worker: &mut WorkerHandle,
    control: &GlobalControl,
) -> Result<(), Diagnostic> {
    let mut next = Some(parent);
    while let Some(barrier) = next.take() {
        match barriers.finish_spilled_chunk(barrier)? {
            Some(BarrierRelease::Resume(resume)) => {
                worker.push_local(ParallelTask::PostOrderResume(resume), control);
            }
            Some(BarrierRelease::NotifyParent(parent)) => {
                next = Some(parent);
            }
            None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::context::{PostorderRunContext, WorkerRunContext};
    use super::{WorkerActionSink, run_postorder_root_task, run_preorder_root_serial};
    use crate::eval::{EvalContext, RuntimeStatus};
    use crate::exec::PromptCoordinator;
    use crate::file_output::SharedFileOutputs;
    use crate::parallel::control::GlobalControl;
    use crate::parallel::postorder::BarrierTable;
    use crate::parallel::scheduler::Scheduler;
    use crate::parallel::task::{ParallelTask, PreOrderRootTask};
    use crate::parser::parse_command;
    use crate::planner::plan_command;
    use crate::walker::FsWalkBackend;
    use crossbeam_channel::unbounded;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn preorder_wide_directory_publishes_sibling_chunk_tasks() {
        let root = tempdir().unwrap();
        for index in 0..40 {
            fs::create_dir(root.path().join(format!("dir-{index:02}"))).unwrap();
        }

        let argv = vec![
            root.path().as_os_str().to_os_string(),
            OsString::from("-false"),
        ];
        let plan = plan_command(parse_command(&argv).unwrap(), 4).unwrap();
        let scheduler = Scheduler::new(1);
        let mut worker = scheduler.worker_handle(0);
        let control = Arc::new(GlobalControl::new());
        let (broker, _rx) = unbounded();
        let file_outputs = SharedFileOutputs::open_all(&[]).unwrap();
        let prompt = Arc::new(PromptCoordinator::open_process());
        let mut sink = WorkerActionSink::new(control.clone(), broker, file_outputs, prompt);
        let mut had_runtime_errors = false;

        let status = run_preorder_root_serial(
            &plan,
            &FsWalkBackend,
            PreOrderRootTask::for_path(root.path().to_path_buf(), 0),
            &mut worker,
            &EvalContext::default(),
            &mut sink,
            &mut had_runtime_errors,
        )
        .unwrap();

        assert_eq!(status, RuntimeStatus::default());
        assert!(!had_runtime_errors);
        assert!(matches!(worker.pop(), Some(ParallelTask::SiblingChunk(_))));
    }

    #[test]
    fn postorder_wide_directory_publishes_chunked_sibling_task_with_completion_barrier() {
        let root = tempdir().unwrap();
        for index in 0..40 {
            fs::create_dir(root.path().join(format!("dir-{index:02}"))).unwrap();
        }

        let argv = vec![
            root.path().as_os_str().to_os_string(),
            OsString::from("-depth"),
            OsString::from("-false"),
        ];
        let plan = plan_command(parse_command(&argv).unwrap(), 4).unwrap();
        let scheduler = Scheduler::new(1);
        let mut worker = scheduler.worker_handle(0);
        let control = Arc::new(GlobalControl::new());
        let (broker, _rx) = unbounded();
        let file_outputs = SharedFileOutputs::open_all(&[]).unwrap();
        let prompt = Arc::new(PromptCoordinator::open_process());
        let mut sink = WorkerActionSink::new(control.clone(), broker, file_outputs, prompt);
        let mut had_runtime_errors = false;
        let barriers = BarrierTable::default();
        let eval_context = EvalContext::default();
        let run_context = WorkerRunContext {
            plan: &plan,
            backend: &FsWalkBackend,
            barriers: &barriers,
            eval_context: &eval_context,
        };
        let mut postorder_context = PostorderRunContext {
            run: run_context,
            worker: &mut worker,
            sink: &mut sink,
            had_runtime_errors: &mut had_runtime_errors,
        };

        let status = run_postorder_root_task(
            PreOrderRootTask::for_path(root.path().to_path_buf(), 0),
            &mut postorder_context,
        )
        .unwrap();

        assert_eq!(status, RuntimeStatus::default());
        assert!(!had_runtime_errors);
        assert!(matches!(
            worker.pop(),
            Some(ParallelTask::SiblingChunk(task)) if task.completion_barrier.is_some()
        ));
    }
}
