mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeAction, plan_command};
use support::{action_labels, argv};

#[test]
fn lowers_exec_semicolon_and_plus_into_distinct_runtime_actions() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".",
            "-exec",
            "printf",
            "pre{}post",
            ";",
            "-exec",
            "echo",
            "{}",
            "+",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(
        action_labels(&plan.expr, |action| match action {
            RuntimeAction::ExecImmediate(_) => Some("exec:semicolon"),
            RuntimeAction::ExecBatched(_) => Some("exec:batch"),
            _ => None,
        }),
        vec!["exec:semicolon", "exec:batch"]
    );
}

#[test]
fn explicit_exec_suppresses_implicit_print() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-exec", "echo", "{}", ";"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(
        action_labels(&plan.expr, |action| match action {
            RuntimeAction::ExecImmediate(_) => Some("exec:semicolon"),
            _ => None,
        }),
        vec!["exec:semicolon"]
    );
}

#[test]
fn batched_exec_requires_one_final_standalone_placeholder() {
    for args in [
        vec![".", "-exec", "echo", "pre{}post", "+"],
        vec![".", "-exec", "echo", "{}", "{}", "+"],
        vec![".", "-exec", "echo", "{}", "tail", "+"],
        vec![".", "-exec", "echo", "+"],
    ] {
        let error = plan_command(parse_command(&argv(&args)).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("`-exec ... +`"));
    }
}

#[test]
fn execdir_ok_and_okdir_remain_unsupported() {
    for (args, needle) in [
        (vec![".", "-execdir", "echo", "{}", ";"], "-execdir"),
        (vec![".", "-ok", "echo", "{}", ";"], "-ok"),
        (vec![".", "-okdir", "echo", "{}", ";"], "-okdir"),
    ] {
        let error = plan_command(parse_command(&argv(&args)).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("unsupported"));
        assert!(error.message.contains(needle));
    }
}
