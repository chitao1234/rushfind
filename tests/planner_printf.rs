mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{OutputAction, RuntimeAction, RuntimeExpr, plan_command};
use support::argv;

#[test]
fn printf_counts_as_an_explicit_action_for_implicit_print_suppression() {
    let plan = plan_command(parse_command(&argv(&[".", "-printf", "%p\\n"])).unwrap(), 1).unwrap();
    assert!(!contains_plain_print(&plan.expr));
}

fn contains_plain_print(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_plain_print),
        RuntimeExpr::Or(left, right) => contains_plain_print(left) || contains_plain_print(right),
        RuntimeExpr::Not(inner) => contains_plain_print(inner),
        RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
