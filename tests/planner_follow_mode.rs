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
