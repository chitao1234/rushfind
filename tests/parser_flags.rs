mod support;

use rushfind::ast::{Expr, Predicate};
use rushfind::parser::parse_command;
use support::argv;

#[test]
fn parses_flags_and_reparse_type_predicates() {
    let ast = parse_command(&argv(&[
        ".",
        "-flags",
        "+readonly,nosystem",
        "-reparse-type",
        "symbolic",
    ]))
    .unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Flags("+readonly,nosystem".into())),
            Expr::Predicate(Predicate::ReparseType("symbolic".into())),
        ])
    );
}

#[test]
fn flags_and_reparse_type_require_operands() {
    assert!(parse_command(&argv(&[".", "-flags"])).is_err());
    assert!(parse_command(&argv(&[".", "-reparse-type"])).is_err());
}
