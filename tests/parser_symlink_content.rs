mod support;

use rushfind::ast::{CommandAst, Expr, Predicate};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_lname_and_ilname_predicates_as_raw_patterns() {
    let ast = parse_command(&argv(&[".", "-lname", "[a-z]*", "-ilname", "*REAL*"])).unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::LName {
                    pattern: "[a-z]*".into(),
                    case_insensitive: false,
                }),
                Expr::Predicate(Predicate::LName {
                    pattern: "*REAL*".into(),
                    case_insensitive: true,
                }),
            ]),
        }
    );
}

#[test]
fn reports_missing_argument_for_lname_and_ilname() {
    for flag in ["-lname", "-ilname"] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("missing argument for `{flag}`"))
        );
    }
}
