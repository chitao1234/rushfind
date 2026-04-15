mod support;

use findoxide::parser::parse_command;
use findoxide::perm::{parse_perm_argument, PermMatcher};
use findoxide::planner::{plan_command, RuntimeExpr, RuntimePredicate};
use std::ffi::OsStr;
use support::argv;

#[test]
fn lowers_octal_and_symbolic_perm_matchers() {
    for raw in [
        "754",
        "-g+w,u+w",
        "/u=w,g=w",
        "g=u",
        "u=",
        "-u=",
        "/u=",
        "+t",
        "+X",
    ] {
        let plan = plan_command(parse_command(&argv(&[".", "-perm", raw])).unwrap(), 1).unwrap();
        let expected = parse_perm_argument(OsStr::new(raw)).unwrap();
        assert_eq!(single_perm(&plan.expr), &expected);
    }
}

#[test]
fn rejects_invalid_perm_forms() {
    for raw in ["+111", "-X", "/X", "definitelybad"] {
        let error =
            plan_command(parse_command(&argv(&[".", "-perm", raw])).unwrap(), 1).unwrap_err();
        assert!(error.message.contains("invalid mode"));
    }
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
