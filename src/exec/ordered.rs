use crate::action_output::{RenderedAction, render_action_output};
use crate::diagnostics::{Diagnostic, failed_to_write, runtime_stderr_line};
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, ActionSink, EvalContext, RuntimeStatus};
use crate::file_output::{OrderedFileOutputs, PlannedFileOutput};
use crate::follow::FollowMode;
use crate::output::StdoutSink;
use crate::planner::RuntimeAction;
use std::collections::BTreeMap;
use std::path::Path;

use super::batch::{BatchLimit, ExecBatchKey, PendingBatch, ReadyBatch, fixed_batch_cost};
use super::child::run_ready_batch;
use super::delete::delete_path;
use super::template::BatchedExecAction;
use super::{
    ConfirmOutcome, PromptCoordinator, build_immediate_command, render_prompt_argv,
    run_prepared_inherited,
};

pub struct OrderedActionSink<'a, W: std::io::Write, E: std::io::Write> {
    output: StdoutSink<'a, W>,
    stderr: &'a mut E,
    file_outputs: OrderedFileOutputs,
    pending: BTreeMap<ExecBatchKey, PendingBatch>,
    batch_limit: BatchLimit,
    had_action_failures: bool,
    prompt: PromptCoordinator,
}

impl<'a, W: std::io::Write, E: std::io::Write> OrderedActionSink<'a, W, E> {
    pub fn new(
        stdout: &'a mut W,
        stderr: &'a mut E,
        planned_file_outputs: &[PlannedFileOutput],
    ) -> Result<Self, Diagnostic> {
        Self::with_prompt(
            stdout,
            stderr,
            planned_file_outputs,
            PromptCoordinator::open_process(),
        )
    }

    pub(crate) fn with_prompt(
        stdout: &'a mut W,
        stderr: &'a mut E,
        planned_file_outputs: &[PlannedFileOutput],
        prompt: PromptCoordinator,
    ) -> Result<Self, Diagnostic> {
        Ok(Self {
            output: StdoutSink::new(stdout),
            stderr,
            file_outputs: OrderedFileOutputs::open_all(planned_file_outputs)?,
            pending: BTreeMap::new(),
            batch_limit: BatchLimit::detect(),
            had_action_failures: false,
            prompt,
        })
    }

    fn enqueue(
        &mut self,
        spec: &BatchedExecAction,
        path: &Path,
    ) -> Result<RuntimeStatus, Diagnostic> {
        let key = ExecBatchKey {
            id: spec.id,
            cwd: spec.batch_cwd(path),
        };
        let mut status = RuntimeStatus::default();
        let ready = {
            let batch = self.pending.entry(key.clone()).or_insert_with(|| {
                PendingBatch::new(spec.clone(), self.batch_limit, fixed_batch_cost(spec))
            });

            if !batch.paths.is_empty() && batch.would_overflow(path) {
                Some(batch.take_ready())
            } else {
                None
            }
        };

        if let Some(ready) = ready {
            if !run_ready_batch(&ready, self.stderr)? {
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        let push_result = {
            let batch = self
                .pending
                .get_mut(&key)
                .expect("pending batch must exist");
            batch.push(path)
        };

        match push_result {
            Ok(Some(ready)) => {
                if !run_ready_batch(&ready, self.stderr)? {
                    self.had_action_failures = true;
                    status = status.merge(RuntimeStatus::action_failure());
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.write_diagnostic(format!("rfd: {error}"))?;
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }

    pub fn write_diagnostic(&mut self, message: impl std::fmt::Display) -> Result<(), Diagnostic> {
        self.stderr
            .write_all(&runtime_stderr_line(message))
            .map_err(|error| failed_to_write("stderr", error))
    }

    pub fn flush(&mut self) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = if self.had_action_failures {
            RuntimeStatus::action_failure()
        } else {
            RuntimeStatus::default()
        };
        let pending = std::mem::take(&mut self.pending);
        for (_, batch) in pending {
            if batch.paths.is_empty() {
                continue;
            }

            let ready = ReadyBatch {
                spec: batch.spec,
                paths: batch.paths,
            };
            if !run_ready_batch(&ready, self.stderr)? {
                self.had_action_failures = true;
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }
}

impl<W: std::io::Write, E: std::io::Write> ActionSink for OrderedActionSink<'_, W, E> {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
        context: &EvalContext,
    ) -> Result<ActionOutcome, Diagnostic> {
        if let Some(rendered) = render_action_output(action, entry, follow_mode, context)? {
            match rendered {
                RenderedAction::Stdout(bytes) => self.output.write_bytes(&bytes)?,
                RenderedAction::File { destination, bytes } => {
                    self.file_outputs.write_record(destination, &bytes)?;
                }
            }
            return Ok(ActionOutcome::matched_true());
        }

        match action {
            RuntimeAction::Quit => Ok(ActionOutcome::quit()),
            RuntimeAction::ExecImmediate(spec) => {
                super::child::run_immediate_ordered(spec, entry.path.as_path(), self.stderr)
                    .map(action_success)
            }
            RuntimeAction::ExecPrompt(spec) => {
                let prompt_argv = render_prompt_argv(spec, entry.path.as_path());
                let prepared = build_immediate_command(spec, entry.path.as_path());
                match self
                    .prompt
                    .confirm_prepared(&prompt_argv, &prepared, |prepared| {
                        run_prepared_inherited(prepared, self.stderr)
                    }) {
                    Ok(ConfirmOutcome::Accepted(true)) => Ok(action_success(true)),
                    Ok(ConfirmOutcome::Accepted(false)) => {
                        self.had_action_failures = true;
                        Ok(action_failure(false))
                    }
                    Ok(ConfirmOutcome::Rejected) => Ok(action_success(false)),
                    Err(error) => {
                        self.write_diagnostic(error)?;
                        self.had_action_failures = true;
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
                    self.write_diagnostic(error)?;
                    self.had_action_failures = true;
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

pub(super) fn action_success(matched: bool) -> ActionOutcome {
    ActionOutcome {
        matched,
        status: RuntimeStatus::default(),
    }
}

pub(super) fn action_failure(matched: bool) -> ActionOutcome {
    ActionOutcome {
        matched,
        status: RuntimeStatus::action_failure(),
    }
}
