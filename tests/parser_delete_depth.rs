mod support;

use findoxide::ast::{Action, Expr, Predicate};
use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_depth_as_an_argumentless_primary() {
    let ast = parse_command(&argv(&[".", "-depth", "-print"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Depth),
            Expr::Action(Action::Print),
        ])
    );
}

#[test]
fn parses_delete_as_an_explicit_action() {
    let ast = parse_command(&argv(&[".", "-name", "*.tmp", "-delete"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Name {
                pattern: "*.tmp".into(),
                case_insensitive: false,
            }),
            Expr::Action(Action::Delete),
        ])
    );
}
