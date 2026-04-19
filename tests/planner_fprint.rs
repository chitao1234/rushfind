mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{OutputAction, RuntimeAction, plan_command};
use std::path::PathBuf;
use support::{argv, contains_action};

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
    assert!(!contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::Output(OutputAction::Print)
    )));
}

#[test]
fn fprintf_reuses_printf_planning_and_mount_snapshot_rules() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-fprintf", "report.txt", "%F\\n"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(plan.runtime.mount_snapshot);
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::FilePrintf { .. }
    )));
}
