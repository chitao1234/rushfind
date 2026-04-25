mod support;

use rushfind::parser::parse_command;
use rushfind::planner::plan_command;
use support::{argv, collect_predicate_labels};

#[test]
fn lowering_access_predicates_produces_dedicated_runtime_predicates() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-readable", "-writable", "-executable"])).unwrap(),
        1,
    )
    .unwrap();

    assert!(!plan.runtime.mount_snapshot);
    assert_eq!(
        collect_predicate_labels(&plan.expr),
        vec!["readable", "writable", "executable"]
    );
}

#[test]
fn access_predicates_do_not_request_mount_snapshot_support() {
    let plan = plan_command(parse_command(&argv(&[".", "-readable"])).unwrap(), 1).unwrap();

    assert!(!plan.runtime.mount_snapshot);
}
