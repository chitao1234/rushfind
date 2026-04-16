mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{
    RuntimeAction, RuntimeExpr, RuntimePredicate, TraversalOrder, plan_command,
};
use support::argv;

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
    assert_eq!(action_labels(&plan.expr), vec!["delete"]);
}

#[test]
fn prune_stays_in_expression_when_delete_forces_depth_mode() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-name", "cache", "-prune", "-o", "-delete"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(plan.traversal.order, TraversalOrder::DepthFirstPostOrder);
    assert!(contains_prune(&plan.expr));
}

fn action_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect_actions(expr, &mut labels);
    labels
}

fn collect_actions(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => items.iter().for_each(|item| collect_actions(item, labels)),
        RuntimeExpr::Or(left, right) => {
            collect_actions(left, labels);
            collect_actions(right, labels);
        }
        RuntimeExpr::Not(inner) => collect_actions(inner, labels),
        RuntimeExpr::Action(RuntimeAction::Delete) => labels.push("delete"),
        RuntimeExpr::Action(_) | RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => {}
    }
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
