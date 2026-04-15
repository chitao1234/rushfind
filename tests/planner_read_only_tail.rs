mod support;

use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{plan_command, RuntimeExpr, RuntimePredicate};
use support::argv;

#[test]
fn lowers_empty_and_used_predicates() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-used", "+2", "-empty"])).unwrap(),
        1,
    )
    .unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Used(matcher)
            if matcher.comparison == NumericComparison::GreaterThan(2)
    )));
    assert!(predicates
        .iter()
        .any(|predicate| matches!(predicate, RuntimePredicate::Empty)));
}

fn predicate_items(expr: &RuntimeExpr) -> Vec<RuntimePredicate> {
    let mut predicates = Vec::new();
    collect(expr, &mut predicates);
    predicates
}

fn collect(expr: &RuntimeExpr, predicates: &mut Vec<RuntimePredicate>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect(item, predicates);
            }
        }
        RuntimeExpr::Predicate(predicate) => predicates.push(predicate.clone()),
        RuntimeExpr::Or(_, _)
        | RuntimeExpr::Not(_)
        | RuntimeExpr::Action(_)
        | RuntimeExpr::Barrier => {}
    }
}
