use crate::planner::{RuntimeExpr, RuntimePredicate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CostTier {
    Constant,
    StringOnly,
    RegexString,
    FileType,
    ActiveMetadata,
    PathAccess,
    BirthTime,
    DirectoryRead,
    Expensive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PredicateProfile {
    reorderable: bool,
    cost: CostTier,
}

/// Reorder the side-effect-free predicates inside `-a` chains so that the
/// cheapest checks run first. This mirrors GNU find's default optimization
/// level, which also reorders predicates that cannot change the outcome.
pub fn optimize_read_only_and_chains(expr: RuntimeExpr) -> RuntimeExpr {
    match expr {
        RuntimeExpr::And(items) => {
            RuntimeExpr::and(optimize_and_items(items.iter().cloned().collect()))
        }
        RuntimeExpr::Or(left, right) => RuntimeExpr::or(
            optimize_read_only_and_chains((*left).clone()),
            optimize_read_only_and_chains((*right).clone()),
        ),
        RuntimeExpr::Sequence(items) => RuntimeExpr::sequence(
            items
                .iter()
                .cloned()
                .map(optimize_read_only_and_chains)
                .collect(),
        ),
        RuntimeExpr::Not(inner) => {
            RuntimeExpr::negate(optimize_read_only_and_chains((*inner).clone()))
        }
        other => other,
    }
}

fn optimize_and_items(items: Vec<RuntimeExpr>) -> Vec<RuntimeExpr> {
    let mut optimized = Vec::with_capacity(items.len());
    let mut segment = Vec::new();

    for item in items.into_iter().map(optimize_read_only_and_chains) {
        if is_reorderable_leaf(&item) {
            segment.push(item);
        } else {
            flush_segment(&mut optimized, &mut segment);
            optimized.push(item);
        }
    }

    flush_segment(&mut optimized, &mut segment);
    optimized
}

fn flush_segment(output: &mut Vec<RuntimeExpr>, segment: &mut Vec<RuntimeExpr>) {
    segment.sort_by_key(expr_cost);
    output.append(segment);
}

fn is_reorderable_leaf(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::Predicate(predicate) => predicate_profile(predicate).reorderable,
        RuntimeExpr::And(_)
        | RuntimeExpr::Or(_, _)
        | RuntimeExpr::Sequence(_)
        | RuntimeExpr::Not(_)
        | RuntimeExpr::Action(_)
        | RuntimeExpr::Barrier => false,
    }
}

fn expr_cost(expr: &RuntimeExpr) -> CostTier {
    match expr {
        RuntimeExpr::Predicate(predicate) => predicate_profile(predicate).cost,
        _ => CostTier::Expensive,
    }
}

fn predicate_profile(predicate: &RuntimePredicate) -> PredicateProfile {
    match predicate {
        // `-prune` is only meaningful in its original position relative to the
        // rest of the expression, so it never joins a reorderable segment.
        RuntimePredicate::Prune => PredicateProfile {
            reorderable: false,
            cost: CostTier::Constant,
        },
        RuntimePredicate::True | RuntimePredicate::False => profile(CostTier::Constant),
        RuntimePredicate::Name(_) | RuntimePredicate::Path(_) => profile(CostTier::StringOnly),
        RuntimePredicate::Regex(_) => profile(CostTier::RegexString),
        RuntimePredicate::Type(_) | RuntimePredicate::XType(_) => profile(CostTier::FileType),
        RuntimePredicate::Inum(_)
        | RuntimePredicate::Links(_)
        | RuntimePredicate::SameFile(_)
        | RuntimePredicate::Uid(_)
        | RuntimePredicate::Gid(_)
        | RuntimePredicate::User(_)
        | RuntimePredicate::Group(_)
        | RuntimePredicate::Perm(_)
        | RuntimePredicate::Flags(_)
        | RuntimePredicate::ReparseType(_)
        | RuntimePredicate::Size(_)
        | RuntimePredicate::Used(_)
        | RuntimePredicate::RelativeTime(_)
        | RuntimePredicate::FsType(_) => profile(CostTier::ActiveMetadata),
        RuntimePredicate::Newer(matcher)
            if matcher.current == crate::time::TimestampKind::Birth =>
        {
            profile(CostTier::BirthTime)
        }
        RuntimePredicate::Newer(_) => profile(CostTier::ActiveMetadata),
        RuntimePredicate::Readable | RuntimePredicate::Writable | RuntimePredicate::Executable => {
            profile(CostTier::PathAccess)
        }
        RuntimePredicate::Empty => profile(CostTier::DirectoryRead),
        RuntimePredicate::LName(_) | RuntimePredicate::NoUser | RuntimePredicate::NoGroup => {
            profile(CostTier::Expensive)
        }
    }
}

fn profile(cost: CostTier) -> PredicateProfile {
    PredicateProfile {
        reorderable: true,
        cost,
    }
}
