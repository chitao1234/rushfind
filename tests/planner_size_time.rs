mod support;

use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command, plan_command_with_now};
use findoxide::size::{SizeMatcher, SizeUnit};
use findoxide::time::{
    RelativeTimeMatcher, RelativeTimeUnit, TimeComparison, Timestamp, TimestampKind,
    local_day_start,
};
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

#[test]
fn lowers_relative_time_predicates_with_a_fixed_now_snapshot() {
    let now = Timestamp::new(1_700_000_000, 0);
    let plan = plan_command_with_now(
        parse_command(&argv(&[".", "-mmin", "+5", "-atime", "1"])).unwrap(),
        1,
        now,
    )
    .unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::RelativeTime(RelativeTimeMatcher {
            kind: TimestampKind::Modification,
            unit: RelativeTimeUnit::Minutes,
            comparison: TimeComparison::GreaterThan(5),
            baseline,
        }) if *baseline == now
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::RelativeTime(RelativeTimeMatcher {
            kind: TimestampKind::Access,
            unit: RelativeTimeUnit::Days,
            comparison: TimeComparison::Exactly(1),
            baseline,
        }) if *baseline == now
    )));
}

#[test]
fn daystart_affects_only_later_relative_time_predicates() {
    let now = Timestamp::new(1_700_000_000, 0);
    let plan = plan_command_with_now(
        parse_command(&argv(&[
            ".",
            "-mtime",
            "0",
            "-daystart",
            "-mmin",
            "0",
            "-name",
            "*.rs",
        ]))
        .unwrap(),
        1,
        now,
    )
    .unwrap();
    let daystart = local_day_start(now).unwrap();

    assert_eq!(
        linear_labels(&plan.expr),
        vec!["mtime", "barrier", "name", "mmin", "print"]
    );

    let matchers = relative_time_matchers(&plan.expr);
    assert_eq!(matchers[0].baseline, now);
    assert_eq!(matchers[1].baseline, daystart);
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
        RuntimeExpr::Barrier => {}
    }
}

fn relative_time_matchers(expr: &RuntimeExpr) -> Vec<RelativeTimeMatcher> {
    predicate_items(expr)
        .into_iter()
        .filter_map(|predicate| match predicate {
            RuntimePredicate::RelativeTime(matcher) => Some(matcher),
            _ => None,
        })
        .collect()
}

fn linear_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    match expr {
        RuntimeExpr::And(items) => items.iter().flat_map(linear_labels).collect(),
        RuntimeExpr::Predicate(RuntimePredicate::Name { .. }) => vec!["name"],
        RuntimeExpr::Predicate(RuntimePredicate::Path { .. }) => vec!["path"],
        RuntimeExpr::Predicate(RuntimePredicate::Uid(_)) => vec!["uid"],
        RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(matcher)) => {
            match (matcher.kind, matcher.unit) {
                (TimestampKind::Access, RelativeTimeUnit::Days) => vec!["atime"],
                (TimestampKind::Change, RelativeTimeUnit::Days) => vec!["ctime"],
                (TimestampKind::Modification, RelativeTimeUnit::Days) => vec!["mtime"],
                (TimestampKind::Access, RelativeTimeUnit::Minutes) => vec!["amin"],
                (TimestampKind::Change, RelativeTimeUnit::Minutes) => vec!["cmin"],
                (TimestampKind::Modification, RelativeTimeUnit::Minutes) => vec!["mmin"],
            }
        }
        RuntimeExpr::Predicate(_) => vec!["predicate"],
        RuntimeExpr::Action(_) => vec!["print"],
        RuntimeExpr::Barrier => vec!["barrier"],
        RuntimeExpr::Or(_, _) => vec!["or"],
        RuntimeExpr::Not(_) => vec!["not"],
    }
}
