use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{ActionOutcome, EvalContext, EvalOutcome, RuntimeStatus, evaluate_predicate};
use crate::follow::FollowMode;
use crate::planner::{RuntimeAction, RuntimeExpr};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SubtreeBarrierId(pub(crate) usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryTicket {
    pub(crate) sequence: u64,
    pub(crate) ancestor_barriers: Vec<SubtreeBarrierId>,
    pub(crate) block_on_subtree: Option<SubtreeBarrierId>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionRequest {
    action: RuntimeAction,
    entry: EntryContext,
    follow_mode: FollowMode,
}

impl ActionRequest {
    pub(crate) fn new(action: RuntimeAction, entry: EntryContext, follow_mode: FollowMode) -> Self {
        Self {
            action,
            entry,
            follow_mode,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn path(&self) -> &Path {
        self.entry.path.as_path()
    }

    pub(crate) fn action(&self) -> &RuntimeAction {
        &self.action
    }

    pub(crate) fn entry(&self) -> &EntryContext {
        &self.entry
    }

    pub(crate) fn follow_mode(&self) -> FollowMode {
        self.follow_mode
    }
}

#[derive(Debug, Clone)]
enum EvalContinuation {
    Done,
    AfterAnd {
        items: Arc<[RuntimeExpr]>,
        next_index: usize,
        accumulated: RuntimeStatus,
        parent: Box<EvalContinuation>,
    },
    AfterSequence {
        items: Arc<[RuntimeExpr]>,
        next_index: usize,
        accumulated: RuntimeStatus,
        parent: Box<EvalContinuation>,
    },
    AfterOrLeft {
        right: Arc<RuntimeExpr>,
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
#[allow(clippy::large_enum_variant)]
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
        expr,
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
    expr: &RuntimeExpr,
    entry: EntryContext,
    follow_mode: FollowMode,
    continuation: EvalContinuation,
    context: &EvalContext,
) -> Result<EvalStep, Diagnostic> {
    match expr {
        RuntimeExpr::Predicate(predicate) => {
            let matched = evaluate_predicate(predicate, &entry, follow_mode, context)?;
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
            request: ActionRequest::new(action.clone(), entry.clone(), follow_mode),
            continuation: PendingEntryEval {
                entry,
                follow_mode,
                continuation,
            },
        }),
        RuntimeExpr::And(items) => {
            if let Some(first) = items.first() {
                return step_expr(
                    first,
                    entry,
                    follow_mode,
                    EvalContinuation::AfterAnd {
                        items: items.clone(),
                        next_index: 1,
                        accumulated: RuntimeStatus::default(),
                        parent: Box::new(continuation),
                    },
                    context,
                );
            }

            finish_with_outcome(
                continuation,
                entry,
                follow_mode,
                EvalOutcome {
                    matched: true,
                    status: RuntimeStatus::default(),
                },
                context,
            )
        }
        RuntimeExpr::Sequence(items) => {
            if let Some(first) = items.first() {
                return step_expr(
                    first,
                    entry,
                    follow_mode,
                    EvalContinuation::AfterSequence {
                        items: items.clone(),
                        next_index: 1,
                        accumulated: RuntimeStatus::default(),
                        parent: Box::new(continuation),
                    },
                    context,
                );
            }

            finish_with_outcome(
                continuation,
                entry,
                follow_mode,
                EvalOutcome {
                    matched: true,
                    status: RuntimeStatus::default(),
                },
                context,
            )
        }
        RuntimeExpr::Or(left, right) => step_expr(
            left.as_ref(),
            entry,
            follow_mode,
            EvalContinuation::AfterOrLeft {
                right: right.clone(),
                parent: Box::new(continuation),
            },
            context,
        ),
        RuntimeExpr::Not(inner) => step_expr(
            inner.as_ref(),
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
            items,
            next_index,
            accumulated,
            parent,
        } => {
            let accumulated = accumulated.merge(outcome.status);
            if !outcome.matched || accumulated.is_stop_requested() {
                return finish_with_outcome(
                    *parent,
                    entry,
                    follow_mode,
                    EvalOutcome {
                        matched: outcome.matched,
                        status: accumulated,
                    },
                    context,
                );
            }

            if next_index >= items.len() {
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

            step_expr(
                &items[next_index],
                entry,
                follow_mode,
                EvalContinuation::AfterAnd {
                    items: items.clone(),
                    next_index: next_index + 1,
                    accumulated,
                    parent,
                },
                context,
            )
        }
        EvalContinuation::AfterSequence {
            items,
            next_index,
            accumulated,
            parent,
        } => {
            let accumulated = accumulated.merge(outcome.status);
            if accumulated.is_stop_requested() || next_index >= items.len() {
                return finish_with_outcome(
                    *parent,
                    entry,
                    follow_mode,
                    EvalOutcome {
                        matched: outcome.matched,
                        status: accumulated,
                    },
                    context,
                );
            }

            step_expr(
                &items[next_index],
                entry,
                follow_mode,
                EvalContinuation::AfterSequence {
                    items: items.clone(),
                    next_index: next_index + 1,
                    accumulated,
                    parent,
                },
                context,
            )
        }
        EvalContinuation::AfterOrLeft { right, parent } => {
            if outcome.matched || outcome.status.is_stop_requested() {
                return finish_with_outcome(*parent, entry, follow_mode, outcome, context);
            }

            step_expr(
                right.as_ref(),
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
    fn sequence_continues_after_false_and_returns_last_truth() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::sequence(vec![
            RuntimeExpr::Predicate(RuntimePredicate::False),
            RuntimeExpr::Predicate(RuntimePredicate::True),
        ]);

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        assert!(matches!(step, EvalStep::Complete(outcome) if outcome.matched));
    }

    #[test]
    fn sequence_result_comes_from_last_child() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::sequence(vec![
            RuntimeExpr::Predicate(RuntimePredicate::True),
            RuntimeExpr::Predicate(RuntimePredicate::False),
        ]);

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        assert!(matches!(step, EvalStep::Complete(outcome) if !outcome.matched));
    }

    #[test]
    fn sequence_stops_after_quit_before_later_action() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::sequence(vec![
            RuntimeExpr::Action(RuntimeAction::Quit),
            RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
        ]);

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();
        let EvalStep::PendingAction {
            request,
            continuation,
        } = step
        else {
            panic!("expected pending quit action");
        };
        assert!(matches!(request.action(), RuntimeAction::Quit));

        let complete = resume_entry_eval(
            continuation,
            ActionOutcome {
                matched: true,
                status: RuntimeStatus::stop_requested(),
            },
            &EvalContext::default(),
        )
        .unwrap();

        assert!(matches!(
            complete,
            EvalStep::Complete(outcome)
                if outcome.matched && outcome.status.is_stop_requested()
        ));
    }

    #[test]
    fn exec_false_short_circuits_the_rest_of_an_and_chain_after_resume() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::and(vec![
            RuntimeExpr::Action(RuntimeAction::ExecImmediate(compile_immediate_exec(
                crate::exec::ExecSemantics::Normal,
                &["false".into()],
            ))),
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
        let expr = RuntimeExpr::or(
            RuntimeExpr::Action(RuntimeAction::Delete),
            RuntimeExpr::Predicate(RuntimePredicate::True),
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
    fn quit_action_stops_after_the_current_and_chain() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let expr = RuntimeExpr::and(vec![
            RuntimeExpr::Action(RuntimeAction::Quit),
            RuntimeExpr::Predicate(RuntimePredicate::False),
        ]);

        let step =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();

        let EvalStep::PendingAction { continuation, .. } = step else {
            panic!("expected quit request");
        };

        let complete =
            resume_entry_eval(continuation, ActionOutcome::quit(), &EvalContext::default())
                .unwrap();

        assert!(matches!(complete, EvalStep::Complete(outcome)
            if outcome.matched && outcome.status.is_stop_requested()));
    }

    #[test]
    fn and_chain_with_multiple_actions_resumes_in_original_order() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let entry = EntryContext::new(path.clone(), 0, true);
        let expr = RuntimeExpr::and(vec![
            RuntimeExpr::Predicate(RuntimePredicate::True),
            RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
            RuntimeExpr::Predicate(RuntimePredicate::True),
            RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print0)),
        ]);

        let first =
            begin_entry_eval(&expr, &entry, FollowMode::Physical, &EvalContext::default()).unwrap();
        let EvalStep::PendingAction {
            request,
            continuation,
        } = first
        else {
            panic!("expected first action request");
        };
        assert!(matches!(
            request.action(),
            RuntimeAction::Output(OutputAction::Print)
        ));

        let second = resume_entry_eval(
            continuation,
            ActionOutcome::matched_true(),
            &EvalContext::default(),
        )
        .unwrap();
        let EvalStep::PendingAction {
            request,
            continuation,
        } = second
        else {
            panic!("expected second action request");
        };
        assert!(matches!(
            request.action(),
            RuntimeAction::Output(OutputAction::Print0)
        ));

        let complete = resume_entry_eval(
            continuation,
            ActionOutcome::matched_true(),
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
