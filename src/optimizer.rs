use crate::planner::{RuntimeExpr, RuntimePredicate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Requirement {
    Basename,
    FullPath,
    FileType,
    ActiveMetadata,
    FilesystemInfo,
    PathAccess,
    BirthTime,
    DirectoryRead,
    LinkTarget,
    Nss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CostTier {
    Constant,
    StringOnly,
    FileType,
    ActiveMetadata,
    PathAccess,
    BirthTime,
    DirectoryRead,
    Expensive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PredicateProfile {
    pub(crate) reorderable: bool,
    pub(crate) requirements: &'static [Requirement],
    cost: CostTier,
}

const NONE: &[Requirement] = &[];
const BASENAME: &[Requirement] = &[Requirement::Basename];
const FULL_PATH: &[Requirement] = &[Requirement::FullPath];
const FILE_TYPE: &[Requirement] = &[Requirement::FileType];
const ACTIVE_METADATA: &[Requirement] = &[Requirement::ActiveMetadata];
const FILESYSTEM_INFO: &[Requirement] = &[Requirement::FilesystemInfo];
const PATH_ACCESS: &[Requirement] = &[Requirement::PathAccess];
const BIRTH_TIME: &[Requirement] = &[Requirement::BirthTime];
const DIRECTORY_READ: &[Requirement] = &[Requirement::ActiveMetadata, Requirement::DirectoryRead];
const LINK_TARGET: &[Requirement] = &[Requirement::LinkTarget];
const ACTIVE_METADATA_AND_NSS: &[Requirement] = &[Requirement::ActiveMetadata, Requirement::Nss];

pub fn optimize_read_only_and_chains(expr: RuntimeExpr) -> RuntimeExpr {
    match expr {
        RuntimeExpr::And(items) => RuntimeExpr::And(optimize_and_items(items)),
        RuntimeExpr::Or(left, right) => RuntimeExpr::Or(
            Box::new(optimize_read_only_and_chains(*left)),
            Box::new(optimize_read_only_and_chains(*right)),
        ),
        RuntimeExpr::Not(inner) => RuntimeExpr::Not(inner),
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

pub(crate) fn predicate_profile(predicate: &RuntimePredicate) -> PredicateProfile {
    match predicate {
        RuntimePredicate::Prune => PredicateProfile {
            reorderable: false,
            requirements: NONE,
            cost: CostTier::Constant,
        },
        RuntimePredicate::FsType(_) => profile(FILESYSTEM_INFO, CostTier::ActiveMetadata),
        RuntimePredicate::Readable
        | RuntimePredicate::Writable
        | RuntimePredicate::Executable => profile(PATH_ACCESS, CostTier::PathAccess),
        RuntimePredicate::True | RuntimePredicate::False => profile(NONE, CostTier::Constant),
        RuntimePredicate::Name { .. } => profile(BASENAME, CostTier::StringOnly),
        RuntimePredicate::Path { .. } => profile(FULL_PATH, CostTier::StringOnly),
        RuntimePredicate::Type(_) | RuntimePredicate::XType(_) => {
            profile(FILE_TYPE, CostTier::FileType)
        }
        RuntimePredicate::Empty => profile(DIRECTORY_READ, CostTier::DirectoryRead),
        RuntimePredicate::Inum(_)
        | RuntimePredicate::Links(_)
        | RuntimePredicate::SameFile(_)
        | RuntimePredicate::Uid(_)
        | RuntimePredicate::Gid(_)
        | RuntimePredicate::User(_)
        | RuntimePredicate::Group(_)
        | RuntimePredicate::Perm(_)
        | RuntimePredicate::Size(_)
        | RuntimePredicate::Used(_)
        | RuntimePredicate::RelativeTime(_) => profile(ACTIVE_METADATA, CostTier::ActiveMetadata),
        RuntimePredicate::Newer(matcher)
            if matcher.current == crate::time::TimestampKind::Birth =>
        {
            profile(BIRTH_TIME, CostTier::BirthTime)
        }
        RuntimePredicate::Newer(_) => profile(ACTIVE_METADATA, CostTier::ActiveMetadata),
        RuntimePredicate::LName { .. } => profile(LINK_TARGET, CostTier::Expensive),
        RuntimePredicate::NoUser | RuntimePredicate::NoGroup => {
            profile(ACTIVE_METADATA_AND_NSS, CostTier::Expensive)
        }
    }
}

fn profile(requirements: &'static [Requirement], cost: CostTier) -> PredicateProfile {
    PredicateProfile {
        reorderable: true,
        requirements,
        cost,
    }
}
