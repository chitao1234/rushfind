mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use findoxide::regex_match::RegexDialect;
use support::argv;

#[test]
fn lowering_uses_default_emacs_until_regextype_changes() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".",
            "-regex",
            "\\(src\\)/.*",
            "-regextype",
            "posix-extended",
            "-regex",
            "(README|LICENSE)",
            "-regextype",
            "posix-basic",
            "-regex",
            ".*",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(
        regex_dialects(&plan.expr),
        vec![
            RegexDialect::Emacs,
            RegexDialect::PosixExtended,
            RegexDialect::PosixBasic,
        ]
    );
    assert_eq!(
        linear_labels(&plan.expr),
        vec!["regex", "barrier", "regex", "barrier", "regex", "print"]
    );
}

#[test]
fn named_classes_are_supported_in_gnu_facing_planning() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".",
            "-regextype",
            "posix-extended",
            "-regex",
            ".*[[:alpha:]][[:digit:]]",
            "-regextype",
            "posix-basic",
            "-regex",
            ".*[[:upper:]]",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(
        regex_dialects(&plan.expr),
        vec![RegexDialect::PosixExtended, RegexDialect::PosixBasic]
    );
}

#[test]
fn unsupported_regextype_is_a_planning_error() {
    let error = plan_command(
        parse_command(&argv(&[".", "-regextype", "sed", "-regex", ".*"])).unwrap(),
        1,
    )
    .unwrap_err();

    assert!(
        error
            .message
            .contains("unsupported `-regextype` value `sed`")
    );
    assert!(error.message.contains("emacs"));
    assert!(error.message.contains("posix-extended"));
    assert!(error.message.contains("posix-basic"));
    assert!(error.message.contains("rust"));
    assert!(error.message.contains("pcre2"));
}

#[test]
fn pcre2_regextype_lowers_into_pcre2_matchers() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".",
            "-regextype",
            "pcre2",
            "-regex",
            ".*/(?:src|docs)/.+\\.(?:rs|txt)",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert_eq!(regex_dialects(&plan.expr), vec![RegexDialect::Pcre2]);
}

#[cfg(unix)]
#[test]
fn non_utf8_regex_patterns_are_accepted_by_planning() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let rust_argv = vec![
        OsString::from("."),
        OsString::from("-regextype"),
        OsString::from("rust"),
        OsString::from("-regex"),
        OsString::from_vec(vec![b'.', b'*', b'/', b'f', b'o', b'o', 0xff]),
    ];
    let rust_plan = plan_command(parse_command(&rust_argv).unwrap(), 1).unwrap();
    assert_eq!(regex_dialects(&rust_plan.expr), vec![RegexDialect::Rust]);

    let gnu_argv = vec![
        OsString::from("."),
        OsString::from("-regextype"),
        OsString::from("posix-extended"),
        OsString::from("-regex"),
        OsString::from_vec(vec![b'.', b'*', b'/', b'f', b'o', b'o', 0xfe]),
    ];
    let gnu_plan = plan_command(parse_command(&gnu_argv).unwrap(), 1).unwrap();
    assert_eq!(
        regex_dialects(&gnu_plan.expr),
        vec![RegexDialect::PosixExtended]
    );
}

#[test]
fn gnu_facing_dialects_report_unsupported_constructs_clearly() {
    for (dialect_name, pattern) in [("posix-extended", "[[.ch.]]"), ("posix-basic", "[[=a=]]")] {
        let error = plan_command(
            parse_command(&argv(&[".", "-regextype", dialect_name, "-regex", pattern])).unwrap(),
            1,
        )
        .unwrap_err();

        assert!(error.message.contains(dialect_name));
        assert!(error.message.contains("unsupported construct"));
    }
}

#[test]
fn gnu_foundation_planning_accepts_backreferences_and_gnu_escapes() {
    for (dialect_name, pattern) in [
        ("posix-basic", r".*/\(.\)\1"),
        ("posix-extended", r".*/(.)\1"),
        ("posix-extended", r".*/\<foo\>"),
    ] {
        let plan = plan_command(
            parse_command(&argv(&[".", "-regextype", dialect_name, "-regex", pattern])).unwrap(),
            1,
        )
        .unwrap();

        assert_eq!(regex_dialects(&plan.expr).len(), 1);
    }
}

#[test]
fn emacs_followup_planning_accepts_backreferences() {
    for pattern in [r".*/\(.\)\1", r".*/\(ab\|cd\)\1"] {
        let plan = plan_command(
            parse_command(&argv(&[".", "-regextype", "emacs", "-regex", pattern])).unwrap(),
            1,
        )
        .unwrap();

        assert_eq!(regex_dialects(&plan.expr), vec![RegexDialect::Emacs]);
    }
}

#[test]
fn gnu_review_followup_planning_rejects_backward_ranges() {
    for dialect_name in ["posix-basic", "posix-extended"] {
        let error = plan_command(
            parse_command(&argv(&[".", "-regextype", dialect_name, "-regex", r".*/[z-a]"]))
                .unwrap(),
            1,
        )
        .unwrap_err();

        assert!(error.message.contains(dialect_name));
        assert!(error.message.contains("invalid range"));
    }
}

fn regex_dialects(expr: &RuntimeExpr) -> Vec<RegexDialect> {
    let mut out = Vec::new();
    collect_regex_dialects(expr, &mut out);
    out
}

fn collect_regex_dialects(expr: &RuntimeExpr, out: &mut Vec<RegexDialect>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect_regex_dialects(item, out);
            }
        }
        RuntimeExpr::Predicate(RuntimePredicate::Regex(matcher)) => out.push(matcher.dialect()),
        RuntimeExpr::Or(left, right) => {
            collect_regex_dialects(left, out);
            collect_regex_dialects(right, out);
        }
        RuntimeExpr::Not(inner) => collect_regex_dialects(inner, out),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => {}
    }
}

fn linear_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect_linear_labels(expr, &mut labels);
    labels
}

fn collect_linear_labels(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect_linear_labels(item, labels);
            }
        }
        RuntimeExpr::Predicate(RuntimePredicate::Regex(_)) => labels.push("regex"),
        RuntimeExpr::Predicate(_) => labels.push("predicate"),
        RuntimeExpr::Action(_) => labels.push("print"),
        RuntimeExpr::Barrier => labels.push("barrier"),
        RuntimeExpr::Or(_, _) => labels.push("or"),
        RuntimeExpr::Not(_) => labels.push("not"),
    }
}
