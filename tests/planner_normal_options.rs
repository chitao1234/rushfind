mod support;

use rushfind::ast::{DebugOption, WarningMode};
use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeExpr, plan_command};
use std::ffi::OsString;
use support::argv;

fn contains_barrier(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::Barrier => true,
        RuntimeExpr::And(items) | RuntimeExpr::Sequence(items) => {
            items.iter().any(contains_barrier)
        }
        RuntimeExpr::Or(left, right) => contains_barrier(left) || contains_barrier(right),
        RuntimeExpr::Not(inner) => contains_barrier(inner),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) => false,
    }
}

#[test]
fn lowers_compatibility_options_to_runtime_barriers() {
    let ast = parse_command(&argv(&[".", "-noleaf", "-ignore_readdir_race"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert!(contains_barrier(&plan.expr));
}

#[test]
fn compatibility_options_do_not_suppress_implicit_print() {
    let ast = parse_command(&argv(&[".", "-noleaf"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    let RuntimeExpr::And(items) = &plan.expr else {
        panic!(
            "expected implicit-print and expression chain, got {:?}",
            plan.expr
        );
    };
    assert!(
        items
            .iter()
            .any(|item| matches!(item, RuntimeExpr::Barrier))
    );
    assert!(
        items
            .iter()
            .any(|item| matches!(item, RuntimeExpr::Action(_)))
    );
}

#[test]
fn copies_recorded_compatibility_options_to_execution_plan() {
    let ast = parse_command(&argv(&[
        "-O4",
        "-D",
        "search,unknown",
        ".",
        "-nowarn",
        "-ignore_readdir_race",
        "-noleaf",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(plan.compatibility_options.optimizer_level, Some(4));
    assert_eq!(
        plan.compatibility_options.debug_options,
        vec![DebugOption::Search]
    );
    assert_eq!(
        plan.compatibility_options.unknown_debug_options,
        vec![OsString::from("unknown")]
    );
    assert_eq!(plan.compatibility_options.warning_mode, WarningMode::NoWarn);
    assert_eq!(plan.compatibility_options.ignore_readdir_race, Some(true));
    assert!(plan.compatibility_options.noleaf);
}
