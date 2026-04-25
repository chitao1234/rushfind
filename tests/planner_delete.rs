mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{ExecutionMode, RuntimeExpr, TraversalOrder, plan_command};
use support::{argv, collect_action_labels, collect_predicate_labels};

#[test]
fn explicit_depth_hoists_post_order_without_leaving_a_runtime_leaf() {
    let plan = plan_command(parse_command(&argv(&[".", "-depth", "-print"])).unwrap(), 1).unwrap();

    assert_eq!(plan.traversal.order, TraversalOrder::DepthFirstPostOrder);
    assert!(!contains_depth_runtime_leaf(&plan.expr));
}

#[test]
fn delete_forces_post_order_and_suppresses_implicit_print() {
    let plan = plan_command(parse_command(&argv(&[".", "-delete"])).unwrap(), 1).unwrap();

    assert_eq!(plan.traversal.order, TraversalOrder::DepthFirstPostOrder);
    assert_eq!(collect_action_labels(&plan.expr), vec!["delete"]);
}

#[test]
fn prune_stays_in_expression_when_delete_forces_depth_mode() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-name", "cache", "-prune", "-o", "-delete"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(plan.traversal.order, TraversalOrder::DepthFirstPostOrder);
    assert!(collect_predicate_labels(&plan.expr).contains(&"prune"));
}

#[test]
fn delete_keeps_parallel_mode_available_when_workers_are_greater_than_one() {
    let plan = plan_command(parse_command(&argv(&[".", "-delete"])).unwrap(), 4).unwrap();

    assert_eq!(plan.mode, ExecutionMode::ParallelRelaxed);
}

fn contains_depth_runtime_leaf(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_depth_runtime_leaf),
        RuntimeExpr::Or(left, right) => {
            contains_depth_runtime_leaf(left) || contains_depth_runtime_leaf(right)
        }
        RuntimeExpr::Not(inner) => contains_depth_runtime_leaf(inner),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
