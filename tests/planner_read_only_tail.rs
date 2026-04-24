mod support;

use rushfind::birth::read_birth_time;
use rushfind::literal_time::parse_literal_time;
use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use rushfind::time::{NewerMatcher, TimeComparison, TimestampKind};
use std::ffi::OsStr;
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
    let literal_flag = if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        "-newermt"
    } else {
        "-newerBt"
    };

    for raw in [
        "@1700000000.25",
        "2026-04-15",
        "20260415",
        "2026-04-15 1234",
        "20260415 1234",
        "20260415T12:34:56.25",
        "2026-04-15T12:34:56Z",
        "2026-04-15T12:34:56+08",
        "2026-04-15T12:34:56+0800",
        "2026-04-15T12:34:56+08:00",
    ] {
        let expected = parse_literal_time(OsStr::new(raw)).unwrap();
        let literal = plan_command(parse_command(&argv(&[".", literal_flag, raw])).unwrap(), 1)
            .unwrap();

        assert!(
            predicate_items(&literal.expr)
                .iter()
                .any(|predicate| matches!(
                    predicate,
                    RuntimePredicate::Newer(NewerMatcher {
                        current,
                        reference,
                    }) if *current == if literal_flag == "-newerBt" {
                        TimestampKind::Birth
                    } else {
                        TimestampKind::Modification
                    } && *reference == expected
                )),
            "{raw}"
        );
    }

    if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        return;
    }

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
    let literal_flag = if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        "-newermt"
    } else {
        "-newerBt"
    };

    for (flag, arg) in [
        ("-newertm", "ref"),
        (literal_flag, "yesterday"),
        (literal_flag, "2026-04"),
        (literal_flag, "2026-04-15T12:34.5"),
        (literal_flag, "2026-04-15T1234"),
        (literal_flag, "202604151234"),
        (literal_flag, "20260415 123456"),
        (literal_flag, "20260415T12:34Z"),
        (literal_flag, "20260415T12:34:56+08:00"),
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
    let Some(path) = support::existing_path_without_birth_time() else {
        return;
    };

    let error = plan_command(
        parse_command(&[
            ".".into(),
            "-newermB".into(),
            path.as_os_str().to_os_string(),
        ])
        .unwrap(),
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
            for item in items.iter() {
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
