mod support;

use findoxide::birth::read_birth_time;
use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use findoxide::time::{NewerMatcher, TimeComparison, Timestamp, TimestampKind};
use std::fs;
use support::argv;
use tempfile::tempdir;

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
            if matcher.comparison == TimeComparison::GreaterThan("2".parse().unwrap())
    )));
    assert!(
        predicates
            .iter()
            .any(|predicate| matches!(predicate, RuntimePredicate::Empty))
    );
}

#[test]
fn lowers_supported_birth_and_literal_newerxy_forms() {
    let literal = plan_command(
        parse_command(&argv(&[".", "-newerBt", "@1700000000.25"])).unwrap(),
        1,
    )
    .unwrap();
    assert!(
        predicate_items(&literal.expr)
            .iter()
            .any(|predicate| matches!(
                predicate,
                RuntimePredicate::Newer(NewerMatcher {
                    current: TimestampKind::Birth,
                    reference,
                }) if *reference == Timestamp::new(1_700_000_000, 250_000_000)
            ))
    );

    let root = tempdir().unwrap();
    let reference = root.path().join("reference.txt");
    fs::write(&reference, "reference\n").unwrap();
    let Some(expected_birth) = read_birth_time(&reference, true).unwrap() else {
        return;
    };

    let plan = plan_command(
        parse_command(&[
            ".".into(),
            "-newermB".into(),
            reference.as_os_str().to_os_string(),
        ])
        .unwrap(),
        1,
    )
    .unwrap();
    assert!(predicate_items(&plan.expr).iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Newer(NewerMatcher {
            current: TimestampKind::Modification,
            reference,
        }) if *reference == expected_birth
    )));
}

#[test]
fn rejects_invalid_current_t_and_unsupported_literal_forms() {
    for (flag, arg) in [
        ("-newertm", "ref"),
        ("-newerBt", "yesterday"),
        ("-newerBt", "2026-04"),
    ] {
        let error = plan_command(parse_command(&argv(&[".", flag, arg])).unwrap(), 1).unwrap_err();
        assert!(
            error.message.contains("invalid `-newerXY`")
                || error.message.contains("unsupported literal time format")
        );
    }
}

#[test]
fn reference_birth_time_unavailability_is_a_planning_error() {
    let error = plan_command(
        parse_command(&argv(&[".", "-newermB", "/proc/self/stat"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(error.message.contains("birth time"));
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
