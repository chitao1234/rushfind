use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::follow::FollowMode;
use crate::planner::{RuntimeExpr, RuntimePredicate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TraversalControl {
    pub(crate) matched: bool,
    pub(crate) prune: bool,
}

pub(crate) fn evaluate_for_traversal(
    expr: &RuntimeExpr,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<TraversalControl, Diagnostic> {
    match expr {
        RuntimeExpr::And(items) => {
            let mut verdict = TraversalControl {
                matched: true,
                prune: false,
            };

            for item in items {
                let next = evaluate_for_traversal(item, entry, follow_mode)?;
                verdict.prune |= next.prune;
                if !next.matched {
                    verdict.matched = false;
                    return Ok(verdict);
                }
            }

            Ok(verdict)
        }
        RuntimeExpr::Or(left, right) => {
            let left = evaluate_for_traversal(left, entry, follow_mode)?;
            if left.matched {
                return Ok(left);
            }

            let right = evaluate_for_traversal(right, entry, follow_mode)?;
            Ok(TraversalControl {
                matched: right.matched,
                prune: left.prune || right.prune,
            })
        }
        RuntimeExpr::Not(inner) => {
            let inner = evaluate_for_traversal(inner, entry, follow_mode)?;
            Ok(TraversalControl {
                matched: !inner.matched,
                prune: inner.prune,
            })
        }
        RuntimeExpr::Predicate(RuntimePredicate::Prune) => Ok(TraversalControl {
            matched: true,
            prune: entry.active_kind(follow_mode)? == EntryKind::Directory,
        }),
        RuntimeExpr::Predicate(predicate) => Ok(TraversalControl {
            matched: crate::eval::evaluate_predicate(predicate, entry, follow_mode)?,
            prune: false,
        }),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => Ok(TraversalControl {
            matched: true,
            prune: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{TraversalControl, evaluate_for_traversal};
    use crate::entry::EntryContext;
    use crate::follow::FollowMode;
    use crate::planner::{OutputAction, RuntimeExpr, RuntimePredicate};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn traversal_control_tracks_boolean_truth_and_prune_flag() {
        let root = tempdir().unwrap();
        let dir = root.path().join("vendor");
        let file = root.path().join("vendor.rs");
        fs::create_dir(&dir).unwrap();
        fs::write(&file, "fn main() {}\n").unwrap();

        let dir_entry = EntryContext::new(dir, 0, true);
        let file_entry = EntryContext::new(file, 0, true);
        let verdict = evaluate_for_traversal(
            &RuntimeExpr::Predicate(RuntimePredicate::Prune),
            &dir_entry,
            FollowMode::Physical,
        )
        .unwrap();
        assert_eq!(
            verdict,
            TraversalControl {
                matched: true,
                prune: true,
            }
        );

        let file_verdict = evaluate_for_traversal(
            &RuntimeExpr::Predicate(RuntimePredicate::Prune),
            &file_entry,
            FollowMode::Physical,
        )
        .unwrap();
        assert_eq!(
            file_verdict,
            TraversalControl {
                matched: true,
                prune: false,
            }
        );
    }

    #[test]
    fn negation_and_actions_do_not_erase_prune_intent() {
        let root = tempdir().unwrap();
        let dir = root.path().join("vendor");
        fs::create_dir(&dir).unwrap();
        let entry = EntryContext::new(dir, 0, true);

        let expr = RuntimeExpr::And(vec![
            RuntimeExpr::Not(Box::new(RuntimeExpr::Predicate(RuntimePredicate::Prune))),
            RuntimeExpr::Action(OutputAction::Print0),
        ]);

        let verdict = evaluate_for_traversal(&expr, &entry, FollowMode::Physical).unwrap();
        assert_eq!(
            verdict,
            TraversalControl {
                matched: false,
                prune: true,
            }
        );
    }
}
