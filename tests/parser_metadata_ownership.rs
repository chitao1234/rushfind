mod support;

use rushfind::ast::{CommandAst, Expr, Predicate};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_metadata_ownership_predicates() {
    let ast = parse_command(&argv(&[
        ".", "-uid", "+42", "-gid", "-2", "-user", "alice", "-group", "staff", "-nouser",
        "-nogroup",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Uid("+42".into())),
                Expr::Predicate(Predicate::Gid("-2".into())),
                Expr::Predicate(Predicate::User("alice".into())),
                Expr::Predicate(Predicate::Group("staff".into())),
                Expr::Predicate(Predicate::NoUser),
                Expr::Predicate(Predicate::NoGroup),
            ]),
        }
    );
}

#[test]
fn reports_missing_argument_for_metadata_ownership_predicates() {
    for flag in ["-uid", "-gid", "-user", "-group"] {
        let error = parse_command(&argv(&[".", flag])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("missing argument for `{flag}`"))
        );
    }
}

#[test]
fn reports_malformed_uid_and_gid_numeric_arguments() {
    for (flag, value) in [("-uid", "+"), ("-gid", "--2"), ("-uid", "abc")] {
        let error = parse_command(&argv(&[".", flag, value])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("invalid numeric argument for `{flag}`"))
        );
    }
}
