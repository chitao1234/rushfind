mod support;

use rushfind::ast::{Action, Expr};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_ls_and_fls_actions() {
    let ls = parse_command(&argv(&[".", "-ls"])).unwrap();
    assert!(matches!(ls.expr, Expr::Action(Action::Ls)));

    let fls = parse_command(&argv(&[".", "-fls", "report.txt"])).unwrap();
    assert!(matches!(
        fls.expr,
        Expr::Action(Action::Fls { ref path }) if path == &PathBuf::from("report.txt")
    ));
}

#[test]
fn reports_missing_fls_argument() {
    let error = parse_command(&argv(&[".", "-fls"])).unwrap_err();
    assert!(error.message.contains("missing argument for `-fls`"));
}
