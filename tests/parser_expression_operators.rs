mod support;

use rushfind::ast::{Action, CommandAst, Expr, Predicate};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_comma_as_lowest_precedence_operator() {
    let ast = parse_command(&argv(&[
        ".", "-name", "a", "-o", "-name", "b", ",", "-print",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::Sequence(vec![
                Expr::Or(
                    Box::new(Expr::Predicate(Predicate::Name {
                        pattern: "a".into(),
                        case_insensitive: false,
                    })),
                    Box::new(Expr::Predicate(Predicate::Name {
                        pattern: "b".into(),
                        case_insensitive: false,
                    })),
                ),
                Expr::Action(Action::Print),
            ]),
        }
    );
}

#[test]
fn parses_parenthesized_sequence_as_primary() {
    let ast = parse_command(&argv(&[
        ".", "(", "-print", ",", "-false", ")", "-o", "-true",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::Or(
            Box::new(Expr::Sequence(vec![
                Expr::Action(Action::Print),
                Expr::Predicate(Predicate::False),
            ])),
            Box::new(Expr::Predicate(Predicate::True)),
        )
    );
}

#[test]
fn reports_trailing_comma_as_parse_error() {
    let error = parse_command(&argv(&[".", "-print", ","])).unwrap_err();

    assert!(
        error.message.contains("expected predicate or action"),
        "{}",
        error.message
    );
}

#[test]
fn comma_does_not_start_an_implicit_and_operand() {
    let ast = parse_command(&argv(&[".", "-false", ",", "-true"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::Sequence(vec![
            Expr::Predicate(Predicate::False),
            Expr::Predicate(Predicate::True),
        ])
    );
}
