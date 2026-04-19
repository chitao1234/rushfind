mod support;

use rushfind::ast::{Action, Expr, Predicate};
use rushfind::parser::parse_command;
use support::argv;

#[test]
fn parses_prune_and_same_filesystem_controls() {
    let ast = parse_command(&argv(&[
        ".", "-name", "vendor", "-prune", "-o", "-mount", "-print0",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::Or(
            Box::new(Expr::And(vec![
                Expr::Predicate(Predicate::Name {
                    pattern: "vendor".into(),
                    case_insensitive: false,
                }),
                Expr::Predicate(Predicate::Prune),
            ])),
            Box::new(Expr::And(vec![
                Expr::Predicate(Predicate::XDev),
                Expr::Action(Action::Print0),
            ])),
        )
    );
}

#[test]
fn xdev_and_mount_are_argumentless_primaries() {
    parse_command(&argv(&[".", "-xdev", "-print"])).unwrap();
    parse_command(&argv(&[".", "-mount", "-print0"])).unwrap();
}
