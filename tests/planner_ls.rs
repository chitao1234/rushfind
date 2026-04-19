mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeAction, RuntimeExpr, plan_command, plan_command_with_now};
use findoxide::time::Timestamp;
use std::path::PathBuf;
use support::argv;

#[test]
fn ls_family_suppresses_implicit_print_and_deduplicates_destinations() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".",
            "-fls",
            "report.txt",
            "-fprint",
            "report.txt",
            "-ls",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(plan.file_outputs.len(), 1);
    assert_eq!(plan.file_outputs[0].path, PathBuf::from("report.txt"));
    assert!(contains_ls(&plan.expr));
    assert!(contains_file_ls(&plan.expr));
    assert!(!contains_implicit_print(&plan.expr));
}

#[test]
fn ls_plans_capture_the_frozen_now_timestamp() {
    let now = Timestamp::new(1_700_000_000, 250_000_000);
    let plan = plan_command_with_now(parse_command(&argv(&[".", "-ls"])).unwrap(), 1, now).unwrap();

    assert_eq!(plan.runtime.evaluation_now, now);
}

fn contains_ls(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_ls),
        RuntimeExpr::Or(left, right) => contains_ls(left) || contains_ls(right),
        RuntimeExpr::Not(inner) => contains_ls(inner),
        RuntimeExpr::Action(RuntimeAction::Ls) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}

fn contains_file_ls(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_file_ls),
        RuntimeExpr::Or(left, right) => contains_file_ls(left) || contains_file_ls(right),
        RuntimeExpr::Not(inner) => contains_file_ls(inner),
        RuntimeExpr::Action(RuntimeAction::FileLs { .. }) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}

fn contains_implicit_print(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_implicit_print),
        RuntimeExpr::Or(left, right) => {
            contains_implicit_print(left) || contains_implicit_print(right)
        }
        RuntimeExpr::Not(inner) => contains_implicit_print(inner),
        RuntimeExpr::Action(RuntimeAction::Output(_)) => true,
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
