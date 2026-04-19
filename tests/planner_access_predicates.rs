mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimePredicate, plan_command};
use support::{argv, predicate_labels};

#[test]
fn lowering_access_predicates_produces_dedicated_runtime_predicates() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-readable", "-writable", "-executable"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(!plan.runtime.mount_snapshot);
    assert_eq!(
        predicate_labels(&plan.expr, |predicate| match predicate {
            RuntimePredicate::Readable => Some("readable"),
            RuntimePredicate::Writable => Some("writable"),
            RuntimePredicate::Executable => Some("executable"),
            other => panic!("unexpected predicate in access planner test: {other:?}"),
        }),
        vec!["readable", "writable", "executable"]
    );
}

#[test]
fn access_predicates_do_not_request_mount_snapshot_support() {
    let plan = plan_command(parse_command(&argv(&[".", "-readable"])).unwrap(), 1).unwrap();

    assert!(!plan.runtime.mount_snapshot);
}
