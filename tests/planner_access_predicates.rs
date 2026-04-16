mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn lowering_access_predicates_produces_dedicated_runtime_predicates() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-readable", "-writable", "-executable"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(!plan.runtime.mount_snapshot);
    assert_eq!(
        predicate_labels(&plan.expr),
        vec!["readable", "writable", "executable"]
    );
}

#[test]
fn access_predicates_do_not_request_mount_snapshot_support() {
    let plan = plan_command(parse_command(&argv(&[".", "-readable"])).unwrap(), 1).unwrap();

    assert!(!plan.runtime.mount_snapshot);
}

fn predicate_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect_predicate_labels(expr, &mut labels);
    labels
}

fn collect_predicate_labels(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect_predicate_labels(item, labels);
            }
        }
        RuntimeExpr::Or(left, right) => {
            collect_predicate_labels(left, labels);
            collect_predicate_labels(right, labels);
        }
        RuntimeExpr::Not(inner) => collect_predicate_labels(inner, labels),
        RuntimeExpr::Predicate(predicate) => labels.push(match predicate {
            RuntimePredicate::Readable => "readable",
            RuntimePredicate::Writable => "writable",
            RuntimePredicate::Executable => "executable",
            other => panic!("unexpected predicate in access planner test: {other:?}"),
        }),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => {}
    }
}
