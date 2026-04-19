mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{RuntimePredicate, plan_command};
use support::{argv, contains_predicate};

#[test]
fn hoists_same_filesystem_controls_and_keeps_prune_as_a_runtime_leaf() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-mount", "-name", "vendor", "-prune", "-o", "-print",
        ]))
        .unwrap(),
        1,
    )
    .unwrap();

    assert!(plan.traversal.same_file_system);
    assert!(contains_predicate(&plan.expr, |predicate| matches!(
        predicate,
        RuntimePredicate::Prune
    )));
}

#[test]
fn repeated_mount_aliases_are_idempotent() {
    let plan = plan_command(parse_command(&argv(&[".", "-xdev", "-mount"])).unwrap(), 1).unwrap();

    assert!(plan.traversal.same_file_system);
}
