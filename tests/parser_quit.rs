mod support;

use findoxide::ast::{Action, Expr};
use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_quit_as_an_explicit_action() {
    let ast = parse_command(&argv(&[".", "-quit"])).unwrap();
    assert!(matches!(ast.expr, Expr::Action(Action::Quit)));
}

#[test]
fn quit_does_not_consume_following_primaries() {
    let ast = parse_command(&argv(&[".", "-quit", "-print"])).unwrap();

    match ast.expr {
        Expr::And(items) => {
            assert!(matches!(items[0], Expr::Action(Action::Quit)));
            assert!(matches!(items[1], Expr::Action(Action::Print)));
        }
        other => panic!("expected implicit and chain, got {other:?}"),
    }
}
