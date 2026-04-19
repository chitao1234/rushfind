mod support;

use findoxide::birth::read_birth_time;
use findoxide::literal_time::parse_literal_time;
use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command, plan_command_with_now};
use findoxide::size::{SizeMatcher, SizeUnit};
use findoxide::time::{
    NewerMatcher, RelativeTimeMatcher, RelativeTimeUnit, TimeComparison, Timestamp, TimestampKind,
    UsedMatcher, local_day_start,
};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use support::argv;
use tempfile::tempdir;

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
            comparison: TimeComparison::GreaterThan(amount),
            baseline,
            daystart,
        }) if *baseline == now && !daystart && amount == &"5".parse().unwrap()
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::RelativeTime(RelativeTimeMatcher {
            kind: TimestampKind::Access,
            unit: RelativeTimeUnit::Days,
            comparison: TimeComparison::Exactly(amount),
            baseline,
            daystart,
        }) if *baseline == now && !daystart && amount == &"1".parse().unwrap()
    )));
}

#[test]
fn lowers_fractional_relative_time_and_used_predicates() {
    let now = Timestamp::new(1_700_000_000, 0);
    let plan = plan_command_with_now(
        parse_command(&argv(&[
            ".", "-mmin", "0.5", "-mtime", "+1.25", "-used", "-0.75",
        ]))
        .unwrap(),
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
            comparison: TimeComparison::Exactly(amount),
            baseline,
            daystart,
        }) if *baseline == now
            && !daystart
            && amount == &"0.5".parse().unwrap()
    )));

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::RelativeTime(RelativeTimeMatcher {
            kind: TimestampKind::Modification,
            unit: RelativeTimeUnit::Days,
            comparison: TimeComparison::GreaterThan(amount),
            baseline,
            daystart,
        }) if *baseline == now
            && !daystart
            && amount == &"1.25".parse().unwrap()
    )));

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Used(UsedMatcher {
            comparison: TimeComparison::LessThan(amount),
        }) if amount == &"0.75".parse().unwrap()
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
    assert!(!matchers[0].daystart);
    assert_eq!(matchers[1].baseline, daystart);
    assert!(matchers[1].daystart);
}

#[test]
fn lowers_newer_shorthands_and_supported_newerxy_forms() {
    let root = tempdir().unwrap();
    let reference = root.path().join("reference.txt");
    fs::write(&reference, "reference\n").unwrap();
    let metadata = fs::metadata(&reference).unwrap();

    for suffix in ["aa", "ac", "am", "ca", "cc", "cm", "ma", "mc", "mm"] {
        let flag = format!("-newer{suffix}");
        let plan = plan_command(
            parse_command(&[
                ".".into(),
                flag.into(),
                reference.as_os_str().to_os_string(),
            ])
            .unwrap(),
            1,
        )
        .unwrap();
        assert!(
            predicate_items(&plan.expr)
                .into_iter()
                .any(|predicate| matches!(predicate, RuntimePredicate::Newer(_)))
        );
    }

    let plan = plan_command(
        parse_command(&[
            ".".into(),
            "-newer".into(),
            reference.as_os_str().to_os_string(),
            "-anewer".into(),
            reference.as_os_str().to_os_string(),
            "-cnewer".into(),
            reference.as_os_str().to_os_string(),
            "-newerac".into(),
            reference.as_os_str().to_os_string(),
            "-newercm".into(),
            reference.as_os_str().to_os_string(),
        ])
        .unwrap(),
        1,
    )
    .unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Newer(NewerMatcher {
            current: TimestampKind::Modification,
            reference,
        }) if *reference == Timestamp::new(metadata.mtime(), metadata.mtime_nsec() as i32)
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Newer(NewerMatcher {
            current: TimestampKind::Access,
            reference,
        }) if *reference == Timestamp::new(metadata.mtime(), metadata.mtime_nsec() as i32)
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Newer(NewerMatcher {
            current: TimestampKind::Change,
            reference,
        }) if *reference == Timestamp::new(metadata.ctime(), metadata.ctime_nsec() as i32)
    )));
}

#[test]
fn lowers_expanded_literal_t_references_for_non_birth_newerxy() {
    for (flag, raw, current_kind) in [
        ("-newermt", "20260415", TimestampKind::Modification),
        ("-newerat", "2026-04-15 1234", TimestampKind::Access),
        ("-newerct", "20260415T1234", TimestampKind::Change),
        (
            "-newermt",
            "2026-04-15T12:34:56+0800",
            TimestampKind::Modification,
        ),
        (
            "-newermt",
            "2026-04-15T12:34:56.25+08:00",
            TimestampKind::Modification,
        ),
    ] {
        let expected = parse_literal_time(OsStr::new(raw)).unwrap();
        let plan = plan_command(parse_command(&argv(&[".", flag, raw])).unwrap(), 1).unwrap();

        assert!(predicate_items(&plan.expr).iter().any(|predicate| matches!(
            predicate,
            RuntimePredicate::Newer(NewerMatcher {
                current,
                reference,
            }) if *current == current_kind && *reference == expected
        )));
    }
}

#[test]
fn stage9_supports_birth_and_literal_reference_forms() {
    let literal = plan_command(
        parse_command(&argv(&[".", "-newerBt", "@1700000000.5"])).unwrap(),
        1,
    )
    .unwrap();
    assert!(
        predicate_items(&literal.expr)
            .into_iter()
            .any(|predicate| matches!(
                predicate,
                RuntimePredicate::Newer(NewerMatcher {
                    current: TimestampKind::Birth,
                    reference,
                }) if reference == Timestamp::new(1_700_000_000, 500_000_000)
            ))
    );

    let root = tempdir().unwrap();
    let reference = root.path().join("birth-reference.txt");
    fs::write(&reference, "reference\n").unwrap();
    match read_birth_time(&reference, true).unwrap() {
        Some(expected_birth) => {
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

            assert!(
                predicate_items(&plan.expr)
                    .into_iter()
                    .any(|predicate| matches!(
                        predicate,
                        RuntimePredicate::Newer(NewerMatcher {
                            current: TimestampKind::Modification,
                            reference,
                        }) if reference == expected_birth
                    ))
            );
        }
        None => {
            let error = plan_command(
                parse_command(&[
                    ".".into(),
                    "-newermB".into(),
                    reference.as_os_str().to_os_string(),
                ])
                .unwrap(),
                1,
            )
            .unwrap_err();
            assert!(error.message.contains("birth time"));
        }
    }
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
fn rejects_missing_reference_files() {
    let error = plan_command(
        parse_command(&argv(&[".", "-newer", "missing-reference"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(error.message.contains("missing-reference"));
}

fn predicate_items(expr: &RuntimeExpr) -> Vec<RuntimePredicate> {
    let mut predicates = Vec::new();
    collect_predicates(expr, &mut predicates);
    predicates
}

fn collect_predicates(expr: &RuntimeExpr, predicates: &mut Vec<RuntimePredicate>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items.iter() {
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
                (TimestampKind::Birth, _) => vec!["time-birth"],
                (TimestampKind::Change, RelativeTimeUnit::Days) => vec!["ctime"],
                (TimestampKind::Modification, RelativeTimeUnit::Days) => vec!["mtime"],
                (TimestampKind::Access, RelativeTimeUnit::Minutes) => vec!["amin"],
                (TimestampKind::Change, RelativeTimeUnit::Minutes) => vec!["cmin"],
                (TimestampKind::Modification, RelativeTimeUnit::Minutes) => vec!["mmin"],
            }
        }
        RuntimeExpr::Predicate(RuntimePredicate::Newer(matcher)) => match matcher.current {
            TimestampKind::Access => vec!["anewer"],
            TimestampKind::Birth => vec!["newer-birth"],
            TimestampKind::Change => vec!["cnewer"],
            TimestampKind::Modification => vec!["newer"],
        },
        RuntimeExpr::Predicate(_) => vec!["predicate"],
        RuntimeExpr::Action(_) => vec!["print"],
        RuntimeExpr::Barrier => vec!["barrier"],
        RuntimeExpr::Or(_, _) => vec!["or"],
        RuntimeExpr::Not(_) => vec!["not"],
    }
}
