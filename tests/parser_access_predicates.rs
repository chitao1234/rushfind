mod support;

use findoxide::ast::{Action, Expr, Predicate};
use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_access_predicates_as_argumentless_booleans() {
    let ast = parse_command(&argv(&[
        ".",
        "-readable",
        "-writable",
        "-executable",
        "-print0",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Readable),
            Expr::Predicate(Predicate::Writable),
            Expr::Predicate(Predicate::Executable),
            Expr::Action(Action::Print0),
        ])
    );
}

#[test]
fn access_predicates_do_not_consume_following_tokens_as_arguments() {
    let error = parse_command(&argv(&[".", "-readable", "stray-token"])).unwrap_err();

    assert!(
        error
            .message
            .contains("unsupported token in parser subset `stray-token`")
    );
}
