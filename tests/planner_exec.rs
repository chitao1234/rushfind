mod support;

use rushfind::exec::ExecSemantics;
use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeAction, plan_command};
use support::{argv, collect_action_labels, contains_action};

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
        collect_action_labels(&plan.expr),
        vec!["exec-immediate", "exec-batched"]
    );
}

#[test]
fn explicit_exec_suppresses_implicit_print() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-exec", "echo", "{}", ";"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(collect_action_labels(&plan.expr), vec!["exec-immediate"]);
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
fn lowers_exec_and_execdir_into_distinct_semantics() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-exec", "echo", "{}", ";", "-execdir", "echo", "{}", ";", "-execdir", "printf",
            "%s\\n", "{}", "+",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::ExecImmediate(spec) if spec.semantics == ExecSemantics::Normal
    )));
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::ExecImmediate(spec) if spec.semantics == ExecSemantics::DirLocal
    )));
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::ExecBatched(spec) if spec.semantics == ExecSemantics::DirLocal
    )));
    assert!(plan.runtime.execdir_requires_safe_path);
}

#[test]
fn execdir_plus_keeps_the_existing_final_placeholder_rule() {
    for args in [
        vec![".", "-execdir", "echo", "pre{}post", "+"],
        vec![".", "-execdir", "echo", "{}", "{}", "+"],
        vec![".", "-execdir", "echo", "{}", "tail", "+"],
        vec![".", "-execdir", "echo", "+"],
    ] {
        let error = plan_command(parse_command(&argv(&args)).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("`-execdir ... +`"));
    }
}

#[test]
fn ok_and_okdir_lower_to_exec_prompt_actions() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-ok", "echo", "{}", ";", "-okdir", "printf", "%s\\n", "{}", ";",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::ExecPrompt(spec) if spec.semantics == ExecSemantics::Normal
    )));
    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::ExecPrompt(spec) if spec.semantics == ExecSemantics::DirLocal
    )));
    assert!(plan.runtime.messages_locale_required);
    assert!(plan.runtime.execdir_requires_safe_path);
}

#[test]
fn ok_and_okdir_plus_are_rejected_with_explicit_diagnostics() {
    for (args, needle) in [
        (
            vec![".", "-ok", "echo", "{}", "+"],
            "`-ok` only supports the `;` terminator",
        ),
        (
            vec![".", "-okdir", "echo", "{}", "+"],
            "`-okdir` only supports the `;` terminator",
        ),
    ] {
        let error = plan_command(parse_command(&argv(&args)).unwrap(), 1).unwrap_err();
        assert!(error.message.contains(needle));
    }
}
