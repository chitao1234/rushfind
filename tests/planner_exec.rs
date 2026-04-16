mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeAction, RuntimeExpr, plan_command};
use support::argv;

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
        action_labels(&plan.expr),
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

    assert_eq!(action_labels(&plan.expr), vec!["exec:semicolon"]);
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
fn execdir_ok_okdir_and_delete_remain_unsupported() {
    for args in [
        vec![".", "-execdir", "echo", "{}", ";"],
        vec![".", "-ok", "echo", "{}", ";"],
        vec![".", "-okdir", "echo", "{}", ";"],
        vec![".", "-delete"],
    ] {
        let error = plan_command(parse_command(&argv(&args)).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("unsupported in stage13"));
    }
}

fn action_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect(expr, &mut labels);
    labels
}

fn collect(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect(item, labels);
            }
        }
        RuntimeExpr::Or(left, right) => {
            collect(left, labels);
            collect(right, labels);
        }
        RuntimeExpr::Not(inner) => collect(inner, labels),
        RuntimeExpr::Action(action) => labels.push(match action {
            RuntimeAction::Output(_) => "print",
            RuntimeAction::ExecImmediate(_) => "exec:semicolon",
            RuntimeAction::ExecBatched(_) => "exec:batch",
        }),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => {}
    }
}
