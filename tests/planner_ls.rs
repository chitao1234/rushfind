mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeAction, plan_command, plan_command_with_now};
use rushfind::time::Timestamp;
use std::path::PathBuf;
use support::{argv, contains_action};

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
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::Ls
    )));
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::FileLs { .. }
    )));
    assert!(!contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::Output(_)
    )));
}

#[test]
fn ls_plans_capture_the_frozen_now_timestamp() {
    let now = Timestamp::new(1_700_000_000, 250_000_000);
    let plan = plan_command_with_now(parse_command(&argv(&[".", "-ls"])).unwrap(), 1, now).unwrap();

    assert_eq!(plan.runtime.evaluation_now, now);
}
