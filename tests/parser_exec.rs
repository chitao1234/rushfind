mod support;

use findoxide::ast::{Action, Expr};
use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_exec_semicolon_and_plus_forms() {
    let ast = parse_command(&argv(&[
        ".", "-exec", "echo", "{}", ";", "-exec", "echo", "{}", "+",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Action(Action::Exec {
                argv: vec!["echo".into(), "{}".into()],
                batch: false,
            }),
            Expr::Action(Action::Exec {
                argv: vec!["echo".into(), "{}".into()],
                batch: true,
            }),
        ])
    );
}

#[test]
fn parses_ok_and_okdir_semicolon_and_plus_forms() {
    let ast = parse_command(&argv(&[
        ".", "-ok", "echo", "{}", ";", "-okdir", "printf", "%s\\n", "{}", "+",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Action(Action::Ok {
                argv: vec!["echo".into(), "{}".into()],
                batch: false,
            }),
            Expr::Action(Action::OkDir {
                argv: vec!["printf".into(), "%s\\n".into(), "{}".into()],
                batch: true,
            }),
        ])
    );
}

#[test]
fn reports_unterminated_exec_action() {
    let error = parse_command(&argv(&[".", "-exec", "echo", "{}"])).unwrap_err();
    assert!(error.message.contains("unterminated exec-style action"));
}
