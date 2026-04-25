mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{OutputAction, RuntimeAction, RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn lowers_comma_expression_to_runtime_sequence() {
    let ast = parse_command(&argv(&[".", "-false", ",", "-print"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    let RuntimeExpr::Sequence(items) = &plan.expr else {
        panic!("expected runtime sequence, got {:?}", plan.expr);
    };

    assert!(matches!(
        &items[0],
        RuntimeExpr::Predicate(RuntimePredicate::False)
    ));
    assert!(matches!(
        &items[1],
        RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print))
    ));
}

#[test]
fn action_inside_sequence_suppresses_implicit_print() {
    let ast = parse_command(&argv(&[".", "-false", ",", "-print"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    let RuntimeExpr::Sequence(items) = &plan.expr else {
        panic!("expected runtime sequence, got {:?}", plan.expr);
    };

    let print_count = items
        .iter()
        .filter(|item| {
            matches!(
                item,
                RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print))
            )
        })
        .count();
    assert_eq!(print_count, 1);
}
