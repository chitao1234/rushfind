mod support;

use rushfind::ast::{CommandAst, Expr, Predicate};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_family_a_predicates_with_expected_ast_shapes() {
    let ast = parse_command(&argv(&[
        ".",
        "-inum",
        "+42",
        "-links",
        "-2",
        "-samefile",
        "ref-link",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Inum("+42".into())),
                Expr::Predicate(Predicate::Links("-2".into())),
                Expr::Predicate(Predicate::SameFile(PathBuf::from("ref-link"))),
            ]),
        }
    );
}

#[test]
fn reports_missing_argument_for_family_a_predicates() {
    for flag in ["-inum", "-links", "-samefile"] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("missing argument for `{flag}`"))
        );
    }
}

#[test]
fn reports_malformed_gnu_numeric_arguments() {
    for (flag, value) in [("-inum", "+"), ("-links", "--2"), ("-inum", "abc")] {
        let error = parse_command(&argv(&[".", flag, value])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("invalid numeric argument for `{flag}`"))
        );
    }
}
