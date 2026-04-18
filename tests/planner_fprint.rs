mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{OutputAction, RuntimeAction, RuntimeExpr, plan_command};
use std::path::PathBuf;
use support::argv;

#[test]
fn fprint_family_suppresses_implicit_print_and_deduplicates_destinations() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-fprint", "out.txt", "-fprintf", "out.txt", "[%p]\\n", "-fprint0", "nul.bin",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(plan.file_outputs.len(), 2);
    assert_eq!(plan.file_outputs[0].path, PathBuf::from("out.txt"));
    assert_eq!(plan.file_outputs[1].path, PathBuf::from("nul.bin"));
    assert!(!contains_plain_print(&plan.expr));
}

#[test]
fn fprintf_reuses_printf_planning_and_mount_snapshot_rules() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-fprintf", "report.txt", "%F\\n"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(plan.runtime.mount_snapshot);
    assert!(contains_file_printf(&plan.expr));
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

fn contains_file_printf(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_file_printf),
        RuntimeExpr::Or(left, right) => contains_file_printf(left) || contains_file_printf(right),
        RuntimeExpr::Not(inner) => contains_file_printf(inner),
        RuntimeExpr::Action(RuntimeAction::FilePrintf { .. }) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
