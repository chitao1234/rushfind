mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeAction, RuntimeExpr, plan_command};
use support::argv;

#[test]
fn quit_counts_as_an_explicit_action_for_implicit_print_suppression() {
    let plan = plan_command(parse_command(&argv(&[".", "-quit"])).unwrap(), 1).unwrap();
    assert!(matches!(
        plan.expr,
        RuntimeExpr::Action(RuntimeAction::Quit)
    ));
}

#[test]
fn planner_lowers_quit_inside_boolean_flow() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-name", "*.rs", "-quit", "-o", "-print"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(contains_quit(&plan.expr));
}

fn contains_quit(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_quit),
        RuntimeExpr::Or(left, right) => contains_quit(left) || contains_quit(right),
        RuntimeExpr::Not(inner) => contains_quit(inner),
        RuntimeExpr::Action(RuntimeAction::Quit) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
