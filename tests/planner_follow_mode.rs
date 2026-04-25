mod support;

use rushfind::ast::{FileTypeFilter, FileTypeMatcher};
use rushfind::follow::FollowMode;
use rushfind::parser::parse_command;
use rushfind::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn defaults_to_physical_follow_mode() {
    let ast = parse_command(&argv(&[".", "-print"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(plan.follow_mode, FollowMode::Physical);
}

#[test]
fn uses_last_follow_mode_option() {
    let ast = parse_command(&argv(&["-P", "-L", ".", "-print"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(plan.follow_mode, FollowMode::Logical);
}

#[test]
fn lowers_xtype_into_the_runtime_expression_tree() {
    let ast = parse_command(&argv(&[".", "-xtype", "l"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    match plan.expr {
        RuntimeExpr::And(ref items) => {
            assert!(items.iter().any(|item| matches!(
                item,
                RuntimeExpr::Predicate(RuntimePredicate::XType(matcher))
                    if *matcher == FileTypeMatcher::single(FileTypeFilter::Symlink)
            )));
        }
        ref other => panic!("expected conjunction with implicit print, got {other:?}"),
    }
}

fn contains_barrier(expr: &RuntimeExpr) -> bool {
    match expr {
        RuntimeExpr::Barrier => true,
        RuntimeExpr::And(items) | RuntimeExpr::Sequence(items) => {
            items.iter().any(contains_barrier)
        }
        RuntimeExpr::Or(left, right) => contains_barrier(left) || contains_barrier(right),
        RuntimeExpr::Not(inner) => contains_barrier(inner),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) => false,
    }
}

#[test]
fn positional_follow_upgrades_effective_follow_mode_to_logical() {
    let ast = parse_command(&argv(&["-P", ".", "-follow", "-type", "f"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(plan.follow_mode, FollowMode::Logical);
    assert!(plan.compatibility_options.follow);
}

#[test]
fn positional_follow_lowers_to_barrier_and_keeps_implicit_print() {
    let ast = parse_command(&argv(&[".", "-follow"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    let RuntimeExpr::And(items) = &plan.expr else {
        panic!("expected implicit-print conjunction, got {:?}", plan.expr);
    };
    assert!(items.iter().any(contains_barrier));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, RuntimeExpr::Action(_)))
    );
}

#[test]
fn context_is_a_recognized_but_unsupported_predicate() {
    let ast = parse_command(&argv(&[".", "-context", "system_u:object_r:tmp_t:s0"])).unwrap();
    let error = plan_command(ast, 1).unwrap_err();

    assert!(error.message.contains("SELinux"), "{}", error.message);
    assert!(error.message.contains("-context"), "{}", error.message);
}

#[test]
fn solaris_door_type_filters_are_recognized_but_unsupported() {
    for args in [
        vec![".", "-type", "D"],
        vec![".", "-xtype", "D"],
        vec![".", "-type", "f,D"],
    ] {
        let ast = parse_command(&argv(&args)).unwrap();
        let error = plan_command(ast, 1).unwrap_err();

        assert!(
            error.message.contains("Solaris door"),
            "{:?}: {}",
            args,
            error.message
        );
    }
}
