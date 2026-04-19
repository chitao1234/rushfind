mod support;

use rushfind::ast::{Action, Expr};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_fprint_family_actions() {
    let fprint = parse_command(&argv(&[".", "-fprint", "out.txt"])).unwrap();
    assert!(matches!(
        fprint.expr,
        Expr::Action(Action::FPrint { ref path }) if path == &PathBuf::from("out.txt")
    ));

    let fprint0 = parse_command(&argv(&[".", "-fprint0", "out.bin"])).unwrap();
    assert!(matches!(
        fprint0.expr,
        Expr::Action(Action::FPrint0 { ref path }) if path == &PathBuf::from("out.bin")
    ));

    let fprintf = parse_command(&argv(&[".", "-fprintf", "report.txt", "[%p]\\n"])).unwrap();
    assert!(matches!(
        fprintf.expr,
        Expr::Action(Action::FPrintf { ref path, ref format })
            if path == &PathBuf::from("report.txt") && format == "[%p]\\n"
    ));
}

#[test]
fn reports_missing_fprint_family_arguments() {
    assert!(
        parse_command(&argv(&[".", "-fprint"]))
            .unwrap_err()
            .message
            .contains("missing argument for `-fprint`")
    );
    assert!(
        parse_command(&argv(&[".", "-fprint0"]))
            .unwrap_err()
            .message
            .contains("missing argument for `-fprint0`")
    );
    assert!(
        parse_command(&argv(&[".", "-fprintf"]))
            .unwrap_err()
            .message
            .contains("missing argument for `-fprintf`")
    );
    assert!(
        parse_command(&argv(&[".", "-fprintf", "report.txt"]))
            .unwrap_err()
            .message
            .contains("missing argument for `-fprintf`")
    );
}
