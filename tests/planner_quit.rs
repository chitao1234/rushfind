mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeAction, RuntimeExpr, plan_command};
use support::{argv, contains_action};

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

    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::Quit
    )));
}
