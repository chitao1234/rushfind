mod support;

use findoxide::ast::{CommandAst, Expr, Predicate};
use findoxide::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_stage8_size_time_predicates() {
    let ast = parse_command(&argv(&[
        ".",
        "-size",
        "+2M",
        "-mtime",
        "1",
        "-amin",
        "-5",
        "-newer",
        "ref-m",
        "-anewer",
        "ref-a",
        "-cnewer",
        "ref-c",
        "-newerac",
        "ref-xy",
        "-daystart",
        "-mmin",
        "+9",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Size("+2M".into())),
                Expr::Predicate(Predicate::MTime("1".into())),
                Expr::Predicate(Predicate::AMin("-5".into())),
                Expr::Predicate(Predicate::Newer(PathBuf::from("ref-m"))),
                Expr::Predicate(Predicate::ANewer(PathBuf::from("ref-a"))),
                Expr::Predicate(Predicate::CNewer(PathBuf::from("ref-c"))),
                Expr::Predicate(Predicate::NewerXY {
                    current: 'a',
                    reference: 'c',
                    reference_path: PathBuf::from("ref-xy"),
                }),
                Expr::Predicate(Predicate::DayStart),
                Expr::Predicate(Predicate::MMin("+9".into())),
            ]),
        }
    );
}

#[test]
fn reports_missing_arguments_for_stage8_predicates() {
    for flag in [
        "-size",
        "-mtime",
        "-atime",
        "-ctime",
        "-mmin",
        "-amin",
        "-cmin",
        "-newer",
        "-anewer",
        "-cnewer",
        "-neweram",
    ] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(error.message.contains(&format!("missing argument for `{flag}`")));
    }
}

#[test]
fn reports_malformed_relative_time_arguments() {
    for (flag, value) in [("-mtime", "+"), ("-amin", "--2"), ("-cmin", "abc")] {
        let error = parse_command(&argv(&[".", flag, value])).unwrap_err();
        assert!(error.message.contains(&format!("invalid numeric argument for `{flag}`")));
    }
}
