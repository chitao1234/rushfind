mod support;

use findoxide::ast::{Action, Expr, Predicate};
use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_fstype_as_a_normal_predicate() {
    let ast = parse_command(&argv(&[".", "-fstype", "tmpfs", "-print0"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::FsType("tmpfs".into())),
            Expr::Action(Action::Print0),
        ])
    );
}

#[test]
fn reports_missing_argument_for_fstype() {
    let error = parse_command(&argv(&[".", "-fstype"])).unwrap_err();

    assert!(error.message.contains("missing argument for `-fstype`"));
}
