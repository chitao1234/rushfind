mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{ParallelExecutionPolicy, plan_command};
use support::argv;

#[test]
fn print_only_parallel_plan_uses_preorder_fast_path() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-type", "f", "-print"])).unwrap(),
        4,
    )
    .unwrap();

    assert_eq!(
        plan.parallel_policy,
        Some(ParallelExecutionPolicy::PreOrderFastPath)
    );
}

#[test]
fn quit_preorder_parallel_plan_stays_on_fast_path() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-type", "f", "-print", "-quit"])).unwrap(),
        4,
    )
    .unwrap();

    assert_eq!(
        plan.parallel_policy,
        Some(ParallelExecutionPolicy::PreOrderFastPath)
    );
}

#[test]
fn delete_parallel_plan_uses_postorder_subtree_policy() {
    let plan = plan_command(parse_command(&argv(&[".", "-delete"])).unwrap(), 4).unwrap();

    assert_eq!(
        plan.parallel_policy,
        Some(ParallelExecutionPolicy::PostOrderSubtree)
    );
    assert!(plan.action_profile.has_subtree_finalizer);
}

#[test]
fn batched_exec_and_quit_are_classified_for_parallel_workers() {
    let plan = plan_command(
        parse_command(&argv(&[
            ".", "-exec", "printf", "B:%s\\n", "{}", "+", "-quit",
        ]))
        .unwrap(),
        4,
    )
    .unwrap();

    assert!(plan.action_profile.has_local_batched);
    assert!(plan.action_profile.has_global_control);
    assert!(!plan.action_profile.has_subtree_finalizer);
}

#[test]
fn single_worker_plan_has_no_parallel_policy() {
    let plan = plan_command(parse_command(&argv(&[".", "-print"])).unwrap(), 1).unwrap();

    assert_eq!(plan.parallel_policy, None);
}
