mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn hoists_same_filesystem_controls_and_keeps_prune_as_a_runtime_leaf() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-mount", "-name", "vendor", "-prune", "-o", "-print",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert!(plan.traversal.same_file_system);
    assert!(contains_prune(&plan.expr));
}

#[test]
fn repeated_mount_aliases_are_idempotent() {
    let plan = plan_command(parse_command(&argv(&[".", "-xdev", "-mount"])).unwrap(), 1).unwrap();

    assert!(plan.traversal.same_file_system);
}

fn contains_prune(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_prune),
        RuntimeExpr::Or(left, right) => contains_prune(left) || contains_prune(right),
        RuntimeExpr::Not(inner) => contains_prune(inner),
        RuntimeExpr::Predicate(RuntimePredicate::Prune) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
