mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{ExecutionMode, OutputAction, RuntimeExpr, plan_command};
use support::argv;

#[test]
fn injects_implicit_print_when_no_action_is_present() {
    let ast = parse_command(&argv(&[".", "-name", "*.rs"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    match plan.expr {
        RuntimeExpr::And(ref items) => {
            assert!(
                items
                    .iter()
                    .any(|item| matches!(item, RuntimeExpr::Action(OutputAction::Print)))
            );
        }
        ref other => panic!("expected implicit print conjunction, got {other:?}"),
    }
}

#[test]
fn hoists_depth_controls_into_traversal_options() {
    let ast = parse_command(&argv(&[
        ".",
        "-mindepth",
        "1",
        "-maxdepth",
        "2",
        "-name",
        "*.rs",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(plan.traversal.min_depth, 1);
    assert_eq!(plan.traversal.max_depth, Some(2));
}

#[test]
fn rejects_exec_in_read_only_v0() {
    let ast = parse_command(&argv(&[".", "-exec", "echo", "{}", ";"])).unwrap();
    let error = plan_command(ast, 1).unwrap_err();

    assert!(error.message.contains("unsupported in read-only v0"));
    assert!(error.message.contains("-exec"));
}

#[test]
fn chooses_parallel_mode_for_more_than_one_worker() {
    let ast = parse_command(&argv(&[".", "-print"])).unwrap();
    let plan = plan_command(ast, 4).unwrap();

    assert_eq!(plan.mode, ExecutionMode::ParallelRelaxed);
}
