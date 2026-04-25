use crate::planner::{RuntimeExpr, RuntimePredicate, TraversalOrder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitPolicy {
    OrderedSequence,
    Relaxed,
    RelaxedWithSubtreeBarriers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimePolicy {
    pub(crate) requested_workers: usize,
    pub(crate) evaluation_workers: usize,
    pub(crate) commit: CommitPolicy,
}

impl RuntimePolicy {
    pub(crate) fn derive(workers: usize, order: TraversalOrder, ordered_mode: bool) -> Self {
        let host_parallelism = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        Self::derive_with_parallelism(workers, order, ordered_mode, host_parallelism)
    }

    fn derive_with_parallelism(
        workers: usize,
        order: TraversalOrder,
        ordered_mode: bool,
        host_parallelism: usize,
    ) -> Self {
        let requested_workers = workers.max(1);
        let evaluation_workers = if ordered_mode {
            host_parallelism.max(1)
        } else {
            requested_workers
        };
        let commit = if ordered_mode {
            CommitPolicy::OrderedSequence
        } else if order == TraversalOrder::DepthFirstPostOrder {
            CommitPolicy::RelaxedWithSubtreeBarriers
        } else {
            CommitPolicy::Relaxed
        };

        Self {
            requested_workers,
            evaluation_workers,
            commit,
        }
    }

    #[cfg(test)]
    pub(crate) fn derive_for_tests(
        workers: usize,
        order: TraversalOrder,
        ordered_mode: bool,
        host_parallelism: usize,
    ) -> Self {
        Self::derive_with_parallelism(workers, order, ordered_mode, host_parallelism)
    }
}

pub(crate) fn build_traversal_control_plan(
    expr: &RuntimeExpr,
    order: TraversalOrder,
) -> Option<RuntimeExpr> {
    if order != TraversalOrder::PreOrder || !contains_prune(expr) {
        return None;
    }

    Some(project_for_traversal(expr))
}

fn contains_prune(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) | RuntimeExpr::Sequence(items) => items.iter().any(contains_prune),
        RuntimeExpr::Or(left, right) => contains_prune(left) || contains_prune(right),
        RuntimeExpr::Not(inner) => contains_prune(inner),
        RuntimeExpr::Predicate(RuntimePredicate::Prune) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}

fn project_for_traversal(expr: &RuntimeExpr) -> RuntimeExpr {
    match expr {
        RuntimeExpr::And(items) => {
            RuntimeExpr::and(items.iter().map(project_for_traversal).collect())
        }
        RuntimeExpr::Sequence(items) => {
            RuntimeExpr::sequence(items.iter().map(project_for_traversal).collect())
        }
        RuntimeExpr::Or(left, right) => {
            RuntimeExpr::or(project_for_traversal(left), project_for_traversal(right))
        }
        RuntimeExpr::Not(inner) => RuntimeExpr::negate(project_for_traversal(inner)),
        RuntimeExpr::Predicate(predicate) => RuntimeExpr::Predicate(predicate.clone()),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => RuntimeExpr::Barrier,
    }
}

#[cfg(test)]
mod tests {
    use super::{CommitPolicy, RuntimePolicy};
    use crate::planner::TraversalOrder;

    #[test]
    fn ordered_mode_uses_host_parallelism_for_internal_evaluation() {
        let policy = RuntimePolicy::derive_for_tests(1, TraversalOrder::PreOrder, true, 6);
        assert_eq!(policy.requested_workers, 1);
        assert_eq!(policy.evaluation_workers, 6);
        assert_eq!(policy.commit, CommitPolicy::OrderedSequence);
    }
}
