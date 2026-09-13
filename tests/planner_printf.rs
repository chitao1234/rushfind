mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{OutputAction, RuntimeAction, plan_command};
use rushfind::printf::{PrintfAtom, PrintfDirective, PrintfDirectiveKind, compile_printf_program};
use std::ffi::OsStr;
use support::{argv, contains_action};

#[test]
fn supported_printf_formats_lower_as_explicit_actions_with_expected_runtime_requirements() {
    for (format, mount_snapshot) in [
        ("%p\\n", false),
        (r"A\cB", false),
        ("%Y\\n", false),
        ("%S\\n", false),
        ("[%a][%T+]\\n", false),
        ("%F\\n", true),
    ] {
        let plan =
            plan_command(parse_command(&argv(&[".", "-printf", format])).unwrap(), 1).unwrap();
        assert!(!contains_action(&plan.expr, |action| matches!(
            action,
            RuntimeAction::Output(OutputAction::Print)
        )));
        assert_eq!(plan.runtime.mount_snapshot, mount_snapshot, "{format}");
        assert!(plan.startup_warnings.is_empty(), "{format}");
    }
}

#[test]
fn rejects_bad_printf_format_sequences() {
    for (format, needle) in [("%", "malformed -printf format: trailing %")] {
        let error =
            plan_command(parse_command(&argv(&[".", "-printf", format])).unwrap(), 1).unwrap_err();
        assert!(
            error.message.contains(needle),
            "{format} -> {}",
            error.message
        );
    }
}

#[test]
fn printf_unknown_escapes_lower_successfully_and_collect_startup_warnings() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-printf", r"X\qY\xZ"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(plan.startup_warnings.len(), 2);
    assert!(
        plan.startup_warnings[0].contains("warning: unrecognized escape `\\q'"),
        "{:?}",
        plan.startup_warnings
    );
    assert!(
        plan.startup_warnings[1].contains("warning: unrecognized escape `\\x'"),
        "{:?}",
        plan.startup_warnings
    );
}

/// A format that ends on a time family letter is not fatal in GNU find: it
/// warns that another character should follow and emits the letter as it is.
#[test]
fn trailing_time_family_directives_warn_and_lower() {
    for format in ["%A", "%C", "%T", "x%A"] {
        let plan =
            plan_command(parse_command(&argv(&[".", "-printf", format])).unwrap(), 1).unwrap();

        assert_eq!(
            plan.startup_warnings,
            vec![format!(
                "rfd: warning: format directive `{}' should be followed by another character",
                &format[format.len() - 2..]
            )],
            "{format}"
        );
    }
}

#[test]
fn accepts_every_byte_as_a_time_selector_without_warnings() {
    for format in [
        "%TQ", "%A~", "%C7", "%Te", "%Ts", "%Tk", "%Tl", "%Tn", "%Tv", "%TC",
    ] {
        let plan =
            plan_command(parse_command(&argv(&[".", "-printf", format])).unwrap(), 1).unwrap();
        assert!(plan.startup_warnings.is_empty(), "{format}");
    }
}

#[test]
fn unrecognized_printf_directives_lower_successfully_and_collect_startup_warnings() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-printf", "A%QB%5Q"])).unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(
        plan.startup_warnings,
        vec![
            "rfd: warning: unrecognized format directive `%Q'".to_string(),
            "rfd: warning: unrecognized format directive `%Q'".to_string(),
        ]
    );
}

#[test]
fn compiler_prefers_windows_sid_directives_over_numeric_ids() {
    let program = compile_printf_program("-printf", OsStr::new("%US|%GS|%U|%G"))
        .unwrap()
        .program;

    assert!(matches!(
        &program.atoms[0],
        PrintfAtom::Directive(PrintfDirective {
            kind: PrintfDirectiveKind::UserSid,
            ..
        })
    ));
    assert!(matches!(
        &program.atoms[2],
        PrintfAtom::Directive(PrintfDirective {
            kind: PrintfDirectiveKind::GroupSid,
            ..
        })
    ));
    assert!(matches!(
        &program.atoms[4],
        PrintfAtom::Directive(PrintfDirective {
            kind: PrintfDirectiveKind::UserId,
            ..
        })
    ));
    assert!(matches!(
        &program.atoms[6],
        PrintfAtom::Directive(PrintfDirective {
            kind: PrintfDirectiveKind::GroupId,
            ..
        })
    ));
}

#[cfg(not(windows))]
#[test]
fn sid_printf_directives_are_rejected_off_windows() {
    let error = plan_command(
        parse_command(&argv(&[".", "-printf", "[%US][%GS]\\n"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(error.message.contains("%US is only supported on Windows"));
}

#[cfg(windows)]
#[test]
fn sid_printf_directives_plan_on_windows() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-printf", "[%US][%GS]\\n"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(contains_action(&plan.expr, |action| matches!(
        action,
        RuntimeAction::Printf(_)
    )));
}
