#![cfg(unix)]

use crate::support::argv;
use rushfind::parser::parse_command;
use rushfind::perm::{PermMatcher, parse_perm_argument};
use rushfind::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use std::ffi::OsStr;

#[test]
fn lowers_octal_and_symbolic_perm_matchers() {
    for raw in [
        "754", "-g+w,u+w", "/u=w,g=w", "g=u", "u=", "-u=", "/u=", "+t", "+X",
    ] {
        let plan = plan_command(parse_command(&argv(&[".", "-perm", raw])).unwrap(), 1).unwrap();
        let expected = parse_perm_argument(OsStr::new(raw)).unwrap();
        assert_eq!(single_perm(&plan.expr), &expected);
    }
}

#[test]
fn rejects_invalid_perm_forms() {
    for raw in ["10000", "-X", "/X", "definitelybad"] {
        let error =
            plan_command(parse_command(&argv(&[".", "-perm", raw])).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("invalid mode"));
    }
}

#[test]
fn numeric_plus_mode_is_any_bit_matching() {
    let plan = plan_command(parse_command(&argv(&[".", "-perm", "+644"])).unwrap(), 1).unwrap();
    assert!(matches!(
        single_perm(&plan.expr),
        PermMatcher::AnyNonZero(0o644)
    ));
}

fn single_perm(expr: &RuntimeExpr) -> &PermMatcher {
    match expr {
        RuntimeExpr::And(items) => items
            .iter()
            .find_map(|item| match item {
                RuntimeExpr::Predicate(RuntimePredicate::Perm(matcher)) => Some(matcher),
                _ => None,
            })
            .unwrap(),
        RuntimeExpr::Predicate(RuntimePredicate::Perm(matcher)) => matcher,
        _ => panic!("expected perm predicate in {expr:?}"),
    }
}
