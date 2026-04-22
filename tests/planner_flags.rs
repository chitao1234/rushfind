mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn lowers_supported_flags_operand_on_this_host() {
    let raw = if cfg!(windows) {
        "+readonly,nosystem"
    } else if cfg!(target_os = "linux") {
        "+immutable,noappend"
    } else {
        "+uchg,noarch"
    };

    let plan = plan_command(parse_command(&argv(&[".", "-flags", raw])).unwrap(), 1).unwrap();
    assert!(contains_flags_predicate(&plan.expr));
}

#[test]
fn rejects_contradictory_flags_conditions() {
    let raw = if cfg!(windows) {
        "readonly,noreadonly"
    } else if cfg!(target_os = "linux") {
        "immutable,noimmutable"
    } else {
        "uchg,nouchg"
    };

    let error = plan_command(parse_command(&argv(&[".", "-flags", raw])).unwrap(), 1).unwrap_err();
    assert!(error.message.contains("contradict"));
}

#[cfg(not(windows))]
#[test]
fn reparse_type_is_rejected_off_windows() {
    let error = plan_command(
        parse_command(&argv(&[".", "-reparse-type", "symbolic"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(error.message.contains("Windows"));
}

#[cfg(windows)]
#[test]
fn reparse_type_lowers_on_windows() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-reparse-type", "symbolic"])).unwrap(),
        1,
    )
    .unwrap();
    assert!(contains_reparse_predicate(&plan.expr));
}

fn contains_flags_predicate(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_flags_predicate),
        RuntimeExpr::Predicate(RuntimePredicate::Flags(_)) => true,
        RuntimeExpr::Or(left, right) => {
            contains_flags_predicate(left) || contains_flags_predicate(right)
        }
        RuntimeExpr::Not(inner) => contains_flags_predicate(inner),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}

#[cfg(windows)]
fn contains_reparse_predicate(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_reparse_predicate),
        RuntimeExpr::Predicate(RuntimePredicate::ReparseType(_)) => true,
        RuntimeExpr::Or(left, right) => {
            contains_reparse_predicate(left) || contains_reparse_predicate(right)
        }
        RuntimeExpr::Not(inner) => contains_reparse_predicate(inner),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
