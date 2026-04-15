mod support;

use findoxide::ast::{CommandAst, Expr, Predicate};
use findoxide::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_perm_predicates() {
    let ast = parse_command(&argv(&[
        ".",
        "-perm",
        "754",
        "-perm",
        "-g+w,u+w",
        "-perm",
        "/u=w,g=w",
        "-perm",
        "g=u",
        "-perm",
        "+t",
        "-perm",
        "+X",
    ]))
    .unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Perm("754".into())),
                Expr::Predicate(Predicate::Perm("-g+w,u+w".into())),
                Expr::Predicate(Predicate::Perm("/u=w,g=w".into())),
                Expr::Predicate(Predicate::Perm("g=u".into())),
                Expr::Predicate(Predicate::Perm("+t".into())),
                Expr::Predicate(Predicate::Perm("+X".into())),
            ]),
        }
    );
}

#[test]
fn reports_missing_argument_for_perm() {
    let error = parse_command(&argv(&[".", "-perm"])).unwrap_err();
    assert!(error.message.contains("missing argument for `-perm`"));
}
