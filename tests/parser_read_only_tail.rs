mod support;

use findoxide::ast::{CommandAst, Expr, Predicate};
use findoxide::parser::parse_command;
use std::ffi::OsString;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_stage9_read_only_tail_predicates() {
    let ast = parse_command(&argv(&[
        ".",
        "-empty",
        "-used",
        "+2",
        "-newerBt",
        "@1700000000.25",
        "-newermB",
        "ref-birth",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Empty),
                Expr::Predicate(Predicate::Used("+2".into())),
                Expr::Predicate(Predicate::NewerXY {
                    current: 'B',
                    reference: 't',
                    reference_arg: OsString::from("@1700000000.25"),
                }),
                Expr::Predicate(Predicate::NewerXY {
                    current: 'm',
                    reference: 'B',
                    reference_arg: OsString::from("ref-birth"),
                }),
            ]),
        }
    );
}

#[test]
fn reports_missing_arguments_for_stage9_tail_predicates() {
    for flag in ["-used", "-newerBt", "-newermB"] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(error
            .message
            .contains(&format!("missing argument for `{flag}`")));
    }
}

#[test]
fn reports_malformed_used_numeric_arguments() {
    for value in ["+", "--2", "abc"] {
        let error = parse_command(&argv(&[".", "-used", value])).unwrap_err();
        assert!(error
            .message
            .contains("invalid numeric argument for `-used`"));
    }
}
