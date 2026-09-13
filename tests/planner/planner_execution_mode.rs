use crate::support::argv;
use rushfind::parser::parse_command;
use rushfind::planner::{ExecutionMode, TraversalOrder, plan_command};

#[test]
fn print_only_plans_use_the_parallel_engine_when_workers_are_available() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-type", "f", "-print"])).unwrap(),
        4,
    )
    .unwrap();

    assert_eq!(plan.mode, ExecutionMode::ParallelRelaxed);
}

#[test]
fn single_worker_plans_stay_ordered() {
    let plan = plan_command(parse_command(&argv(&[".", "-print"])).unwrap(), 1).unwrap();

    assert_eq!(plan.mode, ExecutionMode::OrderedSingle);
}

#[test]
fn exit_plans_force_ordered_execution() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-name", "stop", "-exit", "17"])).unwrap(),
        4,
    )
    .unwrap();

    assert_eq!(plan.mode, ExecutionMode::OrderedSingle);
}

#[test]
fn sorted_plans_force_ordered_execution() {
    let plan = plan_command(parse_command(&argv(&["-s", ".", "-print"])).unwrap(), 4).unwrap();
    assert_eq!(plan.mode, ExecutionMode::OrderedSingle);
}

#[test]
fn delete_plans_use_depth_first_post_order() {
    let plan = plan_command(parse_command(&argv(&[".", "-delete"])).unwrap(), 4).unwrap();

    assert_eq!(plan.traversal.order, TraversalOrder::DepthFirstPostOrder);
}
