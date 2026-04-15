mod support;

use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use findoxide::size::{SizeMatcher, SizeUnit};
use support::argv;

#[test]
fn lowers_gnu_size_units_into_runtime_matchers() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-size", "+1M", "-size", "4c", "-size", "-2b", "-size", "3w", "-size", "1k",
            "-size", "2G",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::GreaterThan(1),
            unit: SizeUnit::MiB,
        })
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::Exactly(4),
            unit: SizeUnit::Bytes,
        })
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::LessThan(2),
            unit: SizeUnit::Blocks512,
        })
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::Exactly(3),
            unit: SizeUnit::Words2,
        })
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::Exactly(1),
            unit: SizeUnit::KiB,
        })
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Size(SizeMatcher {
            comparison: NumericComparison::Exactly(2),
            unit: SizeUnit::GiB,
        })
    )));
}

#[test]
fn rejects_invalid_size_arguments() {
    for raw in ["+M", "12x", "--1k"] {
        let error =
            plan_command(parse_command(&argv(&[".", "-size", raw])).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("invalid size argument"));
    }
}

fn predicate_items(expr: &RuntimeExpr) -> Vec<RuntimePredicate> {
    let mut predicates = Vec::new();
    collect_predicates(expr, &mut predicates);
    predicates
}

fn collect_predicates(expr: &RuntimeExpr, predicates: &mut Vec<RuntimePredicate>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect_predicates(item, predicates);
            }
        }
        RuntimeExpr::Predicate(predicate) => predicates.push(predicate.clone()),
        RuntimeExpr::Or(_, _) | RuntimeExpr::Not(_) | RuntimeExpr::Action(_) => {}
        RuntimeExpr::TraversalBoundary => {}
    }
}
