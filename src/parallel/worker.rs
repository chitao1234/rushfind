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
use crate::planner::{ExecutionPlan, RuntimeAction};
use crossbeam_channel::Sender;
use std::path::Path;
use std::sync::Arc;

const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;

pub(crate) struct WorkerActionSink {
    control: Arc<GlobalControl>,
    broker: Sender<BrokerMessage>,
    batches: WorkerBatchState,
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
