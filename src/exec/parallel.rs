use crate::action_output::{RenderedAction, render_action_output};
use crate::diagnostics::{Diagnostic, internal_poisoned, runtime_stderr_line};
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, ActionSink, EvalContext, RuntimeStatus};
use crate::follow::FollowMode;
use crate::output::BrokerMessage;
use crate::planner::RuntimeAction;
use crossbeam_channel::Sender;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::batch::{BatchLimit, PendingBatch, ReadyBatch, fixed_batch_cost};
use super::child::{
    run_immediate_parallel, run_parallel_ready_batch, run_prepared_inherited,
    send_broker_message,
};
use super::delete::delete_path;
use super::ordered::{action_failure, action_success};
use super::template::{BatchedExecAction, ExecBatchId};
use super::{ConfirmOutcome, PromptCoordinator, build_immediate_command, render_prompt_argv};

const DEFAULT_SPILL_THRESHOLD: usize = 64 * 1024;

#[derive(Clone)]
pub struct ParallelActionSink {
    broker: Sender<BrokerMessage>,
    shared: Arc<ParallelExecShared>,
}

struct ParallelExecShared {
    pending: Mutex<BTreeMap<ExecBatchId, PendingBatch>>,
    batch_limit: BatchLimit,
    had_action_failures: AtomicBool,
    spill_threshold: usize,
    prompt: PromptCoordinator,
}

impl ParallelActionSink {
    pub fn new(broker: Sender<BrokerMessage>, _workers: usize) -> Result<Self, Diagnostic> {
        Ok(Self {
            broker,
            shared: Arc::new(ParallelExecShared {
                pending: Mutex::new(BTreeMap::new()),
                batch_limit: BatchLimit::detect(),
                had_action_failures: AtomicBool::new(false),
                spill_threshold: DEFAULT_SPILL_THRESHOLD,
                prompt: PromptCoordinator::open_process(),
            }),
        })
    }

    pub fn flush_all(&self) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = if self.shared.had_action_failures.load(Ordering::SeqCst) {
            RuntimeStatus::action_failure()
        } else {
            RuntimeStatus::default()
        };
        let pending = {
            let mut pending = self
                .shared
                .pending
                .lock()
                .map_err(|_| internal_poisoned("parallel exec batch state"))?;
            std::mem::take(&mut *pending)
        };

        for (_, batch) in pending {
            if batch.paths.is_empty() {
                continue;
            }

            let ready = ReadyBatch {
                spec: batch.spec,
                paths: batch.paths,
            };
            if !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)? {
                self.mark_action_failure();
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    fn enqueue(&self, spec: &BatchedExecAction, path: &Path) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = RuntimeStatus::default();
        let (ready, push_result) = {
            let mut pending = self
                .shared
                .pending
                .lock()
                .map_err(|_| internal_poisoned("parallel exec batch state"))?;
            let batch = pending.entry(spec.id).or_insert_with(|| {
                PendingBatch::new(
                    spec.clone(),
                    self.shared.batch_limit,
                    fixed_batch_cost(spec),
                )
            });

            let ready = if !batch.paths.is_empty() && batch.would_overflow(path) {
                Some(batch.take_ready())
            } else {
                None
            };
            let push_result = batch.push(path);
            (ready, push_result)
        };

        if let Some(ready) = ready
            && !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)?
        {
            self.mark_action_failure();
            status = status.merge(RuntimeStatus::action_failure());
        }

        match push_result {
            Ok(Some(ready)) => {
                if !run_parallel_ready_batch(&ready, &self.broker, self.shared.spill_threshold)? {
                    self.mark_action_failure();
                    status = status.merge(RuntimeStatus::action_failure());
                }
            }
            Ok(None) => {}
            Err(error) => {
                send_broker_message(
                    &self.broker,
                    BrokerMessage::Stderr(format!("findoxide: {error}\n").into_bytes()),
                )?;
                self.mark_action_failure();
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    fn mark_action_failure(&self) {
        self.shared
            .had_action_failures
            .store(true, Ordering::SeqCst);
    }

    fn execute_action(
        &self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        if let Some(rendered) = render_action_output(action, entry, follow_mode, context)? {
            return match rendered {
                RenderedAction::Stdout(bytes) => {
                    send_broker_message(&self.broker, BrokerMessage::Stdout(bytes))?;
                    Ok(ActionOutcome::matched_true())
                }
                RenderedAction::File { .. } => Err(Diagnostic::new(
                    "internal error: file-backed output actions are not wired into parallel execution yet",
                    1,
                )),
            };
        }

        match action {
            RuntimeAction::Quit => Ok(ActionOutcome::quit()),
            RuntimeAction::ExecImmediate(spec) => run_immediate_parallel(
                spec,
                entry.path.as_path(),
                &self.broker,
                self.shared.spill_threshold,
            )
            .map(action_success),
            RuntimeAction::ExecPrompt(spec) => {
                let prompt_argv = render_prompt_argv(spec, entry.path.as_path());
                let prepared = build_immediate_command(spec, entry.path.as_path());
                match self
                    .shared
                    .prompt
                    .confirm_prepared(&prompt_argv, &prepared, |prepared| {
                        let mut stderr = std::io::stderr();
                        run_prepared_inherited(prepared, &mut stderr)
                    }) {
                    Ok(ConfirmOutcome::Accepted(true)) => Ok(action_success(true)),
                    Ok(ConfirmOutcome::Accepted(false)) => {
                        self.mark_action_failure();
                        Ok(action_failure(false))
                    }
                    Ok(ConfirmOutcome::Rejected) => Ok(action_success(false)),
                    Err(error) => {
                        send_broker_message(
                            &self.broker,
                            BrokerMessage::Stderr(runtime_stderr_line(error.message)),
                        )?;
                        self.mark_action_failure();
                        Ok(action_failure(false))
                    }
                }
            }
            RuntimeAction::ExecBatched(spec) => Ok(ActionOutcome {
                matched: true,
                status: self.enqueue(spec, entry.path.as_path())?,
            }),
            RuntimeAction::Delete => match delete_path(entry.path.as_path()) {
                Ok(result) => Ok(action_success(result)),
                Err(error) => {
                    send_broker_message(
                        &self.broker,
                        BrokerMessage::Stderr(runtime_stderr_line(error.message)),
                    )?;
                    self.mark_action_failure();
                    Ok(action_failure(false))
                }
            },
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

impl ActionSink for ParallelActionSink {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        self.execute_action(action, entry, follow_mode, context)
    }
}
