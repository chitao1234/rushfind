mod support;

use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn lowering_fstype_requests_mount_snapshot_runtime_support() {
    let plan = plan_command(parse_command(&argv(&[".", "-fstype", "tmpfs"])).unwrap(), 1).unwrap();

    assert!(plan.runtime.mount_snapshot);
    assert!(contains_fstype(&plan.expr));
}

#[test]
fn plans_without_fstype_do_not_request_mount_snapshot_runtime_support() {
    let plan = plan_command(parse_command(&argv(&[".", "-name", "*.rs"])).unwrap(), 1).unwrap();

    assert!(!plan.runtime.mount_snapshot);
}

fn contains_fstype(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::And(items) => items.iter().any(contains_fstype),
        RuntimeExpr::Or(left, right) => contains_fstype(left) || contains_fstype(right),
        RuntimeExpr::Not(inner) => contains_fstype(inner),
        RuntimeExpr::Predicate(RuntimePredicate::FsType(type_name)) => type_name == "tmpfs",
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => false,
    }
}
