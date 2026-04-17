mod support;

use findoxide::ast::{Action, CommandAst, Expr, Predicate};
use findoxide::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_regex_family_and_path_aliases() {
    let ast = parse_command(&argv(&[
        ".",
        "-regex",
        ".*\\.rs",
        "-iregex",
        ".*readme.*",
        "-regextype",
        "rust",
        "-wholename",
        "./src/*",
        "-iwholename",
        "./readme*",
        "-print0",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Regex {
                    pattern: ".*\\.rs".into(),
                    case_insensitive: false,
                }),
                Expr::Predicate(Predicate::Regex {
                    pattern: ".*readme.*".into(),
                    case_insensitive: true,
                }),
                Expr::Predicate(Predicate::RegexType("rust".into())),
                Expr::Predicate(Predicate::Path {
                    pattern: "./src/*".into(),
                    case_insensitive: false,
                }),
                Expr::Predicate(Predicate::Path {
                    pattern: "./readme*".into(),
                    case_insensitive: true,
                }),
                Expr::Action(Action::Print0),
            ]),
        }
    );
}

#[test]
fn reports_missing_arguments_for_regex_family() {
    for flag in [
        "-regex",
        "-iregex",
        "-regextype",
        "-wholename",
        "-iwholename",
    ] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("missing argument for `{flag}`"))
        );
    }
}

#[test]
fn parses_posix_basic_regextype_as_regex_type_argument() {
    let ast = parse_command(&argv(&[".", "-regextype", "posix-basic", "-regex", ".*"])).unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::RegexType("posix-basic".into())),
                Expr::Predicate(Predicate::Regex {
                    pattern: ".*".into(),
                    case_insensitive: false,
                }),
            ]),
        }
    );
}
