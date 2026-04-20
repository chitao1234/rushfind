mod support;

use rushfind::ast::{Action, CommandAst, Expr, FileTypeFilter, Predicate};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_implicit_and_chain() {
    let ast = parse_command(&argv(&[".", "-maxdepth", "2", "-name", "*.rs", "-print"])).unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::MaxDepth(2)),
                Expr::Predicate(Predicate::Name {
                    pattern: "*.rs".into(),
                    case_insensitive: false,
                }),
                Expr::Action(Action::Print),
            ]),
        }
    );
}

#[test]
fn parses_parenthesized_or_expression() {
    let ast = parse_command(&argv(&[
        ".", "(", "-name", "*.rs", "-o", "-name", "*.md", ")", "-type", "f",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Or(
                Box::new(Expr::Predicate(Predicate::Name {
                    pattern: "*.rs".into(),
                    case_insensitive: false,
                })),
                Box::new(Expr::Predicate(Predicate::Name {
                    pattern: "*.md".into(),
                    case_insensitive: false,
                })),
            ),
            Expr::Predicate(Predicate::Type(FileTypeFilter::File)),
        ])
    );
}

#[test]
fn parses_glob_predicates_as_raw_patterns_before_planning() {
    let ast = parse_command(&argv(&[".", "-name", "[A-Z]*", "-ipath", "*/main.rs"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Name {
                pattern: "[A-Z]*".into(),
                case_insensitive: false,
            }),
            Expr::Predicate(Predicate::Path {
                pattern: "*/main.rs".into(),
                case_insensitive: true,
            }),
        ])
    );
}

#[test]
fn parses_unsupported_exec_without_rejecting_it() {
    let ast = parse_command(&argv(&[".", "-exec", "echo", "{}", ";"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::Action(Action::Exec {
            argv: vec!["echo".into(), "{}".into()],
            batch: false,
        })
    );
}
