use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, EvalContext, EvalOutcome, RuntimeStatus, evaluate_predicate};
use crate::follow::FollowMode;
use crate::planner::{RuntimeAction, RuntimeExpr};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionRequest {
    action: RuntimeAction,
    path: PathBuf,
}

impl ActionRequest {
    pub(crate) fn new(action: RuntimeAction, path: PathBuf) -> Self {
        Self { action, path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn action(&self) -> &RuntimeAction {
        &self.action
    }
}

#[derive(Debug, Clone)]
enum EvalContinuation {
    Done,
    AfterAnd {
        remaining: Vec<RuntimeExpr>,
        accumulated: RuntimeStatus,
        parent: Box<EvalContinuation>,
    },
    AfterOrLeft {
        right: RuntimeExpr,
        parent: Box<EvalContinuation>,
    },
    AfterOrRight {
        left_status: RuntimeStatus,
        parent: Box<EvalContinuation>,
    },
    AfterNot {
        parent: Box<EvalContinuation>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingEntryEval {
    entry: EntryContext,
    follow_mode: FollowMode,
    continuation: EvalContinuation,
}

#[derive(Debug, Clone)]
pub(crate) enum EvalStep {
    Complete(EvalOutcome),
    PendingAction {
        request: ActionRequest,
        continuation: PendingEntryEval,
    },
}

#[derive(Debug)]
pub(crate) struct OrderedReadyQueue<T> {
    next_sequence: u64,
    buffered: BTreeMap<u64, T>,
}

impl<T> Default for OrderedReadyQueue<T> {
    fn default() -> Self {
        Self {
            next_sequence: 0,
            buffered: BTreeMap::new(),
        }
    }
}

impl<T> OrderedReadyQueue<T> {
    pub(crate) fn insert(&mut self, sequence: u64, item: T) {
        self.buffered.insert(sequence, item);
    }

    pub(crate) fn pop_next(&mut self) -> Option<T> {
        let item = self.buffered.remove(&self.next_sequence)?;
        self.next_sequence += 1;
        Some(item)
    }
}

pub(crate) fn begin_entry_eval(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<EvalStep, Diagnostic> {
    step_expr(
        expr.clone(),
        entry.clone(),
        follow_mode,
        EvalContinuation::Done,
        context,
    )
}

pub(crate) fn resume_entry_eval(
    pending: PendingEntryEval,
    outcome: ActionOutcome,
    context: &EvalContext,
) -> Result<EvalStep, Diagnostic> {
    finish_with_outcome(
        pending.continuation,
        pending.entry,
        pending.follow_mode,
        EvalOutcome {
            matched: outcome.matched,
            status: outcome.status,
        },
        context,
    )
}

fn step_expr(
    expr: RuntimeExpr,
    entry: EntryContext,
    follow_mode: FollowMode,
    continuation: EvalContinuation,
    context: &EvalContext,
) -> Result<EvalStep, Diagnostic> {
    match expr {
        RuntimeExpr::Predicate(predicate) => {
            let matched = evaluate_predicate(&predicate, &entry, follow_mode, context)?;
            finish_with_outcome(
                continuation,
                entry,
                follow_mode,
                EvalOutcome {
                    matched,
                    status: RuntimeStatus::default(),
                },
                context,
            )
        }
        RuntimeExpr::Action(action) => Ok(EvalStep::PendingAction {
            request: ActionRequest::new(action, entry.path.clone()),
            continuation: PendingEntryEval {
                entry,
                follow_mode,
                continuation,
            },
        }),
        RuntimeExpr::And(mut items) => {
            if items.is_empty() {
                return finish_with_outcome(
                    continuation,
                    entry,
                    follow_mode,
                    EvalOutcome {
                        matched: true,
                        status: RuntimeStatus::default(),
                    },
                    context,
                );
            }

            let first = items.remove(0);
            step_expr(
                first,
                entry,
                follow_mode,
                EvalContinuation::AfterAnd {
                    remaining: items,
                    accumulated: RuntimeStatus::default(),
                    parent: Box::new(continuation),
                },
                context,
            )
        }
        RuntimeExpr::Or(left, right) => step_expr(
            *left,
            entry,
            follow_mode,
            EvalContinuation::AfterOrLeft {
                right: *right,
                parent: Box::new(continuation),
            },
            context,
        ),
        RuntimeExpr::Not(inner) => step_expr(
            *inner,
            entry,
            follow_mode,
            EvalContinuation::AfterNot {
                parent: Box::new(continuation),
            },
            context,
        ),
        RuntimeExpr::Barrier => finish_with_outcome(
            continuation,
            entry,
            follow_mode,
            EvalOutcome {
                matched: true,
                status: RuntimeStatus::default(),
            },
            context,
        ),
    }
}

fn finish_with_outcome(
    continuation: EvalContinuation,
    entry: EntryContext,
    follow_mode: FollowMode,
    outcome: EvalOutcome,
    context: &EvalContext,
) -> Result<EvalStep, Diagnostic> {
    match continuation {
        EvalContinuation::Done => Ok(EvalStep::Complete(outcome)),
        EvalContinuation::AfterAnd {
            mut remaining,
            accumulated,
            parent,
        } => {
            let accumulated = accumulated.merge(outcome.status);
            if !outcome.matched {
                return finish_with_outcome(
                    *parent,
                    entry,
                    follow_mode,
                    EvalOutcome {
                        matched: false,
                        status: accumulated,
                    },
                    context,
                );
            }

            if remaining.is_empty() {
                return finish_with_outcome(
                    *parent,
                    entry,
                    follow_mode,
                    EvalOutcome {
                        matched: true,
                        status: accumulated,
                    },
                    context,
                );
            }

            let next = remaining.remove(0);
            step_expr(
                next,
                entry,
                follow_mode,
                EvalContinuation::AfterAnd {
                    remaining,
                    accumulated,
                    parent,
                },
                context,
            )
        }
        EvalContinuation::AfterOrLeft { right, parent } => {
            if outcome.matched {
                return finish_with_outcome(*parent, entry, follow_mode, outcome, context);
            }

            step_expr(
                right,
                entry,
                follow_mode,
                EvalContinuation::AfterOrRight {
                    left_status: outcome.status,
                    parent,
                },
                context,
            )
        }
        EvalContinuation::AfterOrRight {
            left_status,
            parent,
        } => finish_with_outcome(
            *parent,
            entry,
            follow_mode,
            EvalOutcome {
                matched: outcome.matched,
                status: left_status.merge(outcome.status),
            },
            context,
        ),
        EvalContinuation::AfterNot { parent } => finish_with_outcome(
            *parent,
            entry,
            follow_mode,
            EvalOutcome {
                matched: !outcome.matched,
                status: outcome.status,
            },
            context,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{EvalStep, begin_entry_eval, resume_entry_eval};
    use crate::entry::EntryContext;
    use crate::eval::{ActionOutcome, EvalContext, RuntimeStatus};
    use crate::exec::compile_immediate_exec;
    use crate::follow::FollowMode;
    use crate::planner::{OutputAction, RuntimeAction, RuntimeExpr, RuntimePredicate};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn print_action_yields_a_request_then_completes_true() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path.clone(), 0, true);
        let expr = RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print));

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        let EvalStep::PendingAction {
            request,
            continuation,
        } = step
        else {
            panic!("expected pending action request");
        };
        assert_eq!(request.path(), path.as_path());

        let complete = resume_entry_eval(
            continuation,
            ActionOutcome {
                matched: true,
                status: RuntimeStatus::default(),
            },
            &EvalContext::default(),
        )
        .unwrap();

        assert!(matches!(complete, EvalStep::Complete(outcome) if outcome.matched));
    }

    #[test]
    fn exec_false_short_circuits_the_rest_of_an_and_chain_after_resume() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::And(vec![
            RuntimeExpr::Action(RuntimeAction::ExecImmediate(compile_immediate_exec(&[
                "false".into(),
            ]))),
            RuntimeExpr::Predicate(RuntimePredicate::True),
        ]);

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        let EvalStep::PendingAction { continuation, .. } = step else {
            panic!("expected exec request");
        };

        let complete = resume_entry_eval(
            continuation,
            ActionOutcome {
                matched: false,
                status: RuntimeStatus::default(),
            },
            &EvalContext::default(),
        )
        .unwrap();

        assert!(matches!(complete, EvalStep::Complete(outcome) if !outcome.matched));
    }

    #[test]
    fn delete_false_can_fall_through_the_or_rhs_after_resume() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::Or(
            Box::new(RuntimeExpr::Action(RuntimeAction::Delete)),
            Box::new(RuntimeExpr::Predicate(RuntimePredicate::True)),
        );

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        let EvalStep::PendingAction { continuation, .. } = step else {
            panic!("expected delete request");
        };

        let complete = resume_entry_eval(
            continuation,
            ActionOutcome {
                matched: false,
                status: RuntimeStatus::action_failure(),
            },
            &EvalContext::default(),
        )
        .unwrap();

        assert!(matches!(complete, EvalStep::Complete(outcome) if outcome.matched));
    }

    #[test]
    fn ordered_ready_queue_releases_only_the_next_sequence() {
        let mut queue = super::OrderedReadyQueue::default();
        queue.insert(2, "two");
        assert!(queue.pop_next().is_none());

        queue.insert(0, "zero");
        queue.insert(1, "one");

        assert_eq!(queue.pop_next(), Some("zero"));
        assert_eq!(queue.pop_next(), Some("one"));
        assert_eq!(queue.pop_next(), Some("two"));
    }
}
