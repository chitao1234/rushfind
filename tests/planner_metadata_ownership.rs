mod support;

use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{plan_command, RuntimeExpr, RuntimePredicate};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;
use support::argv;
use tempfile::tempdir;

#[test]
fn lowers_uid_and_gid_into_runtime_numeric_comparisons() {
    let ast = parse_command(&argv(&[".", "-uid", "+42", "-gid", "-2"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Uid(NumericComparison::GreaterThan(42))
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Gid(NumericComparison::LessThan(2))
    )));
}

#[test]
fn lowers_named_and_numeric_user_group_into_exact_ids() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file.txt"), "hello\n").unwrap();
    let metadata = fs::metadata(root.path().join("file.txt")).unwrap();
    let uid = metadata.uid();
    let gid = metadata.gid();
    let user = current_id_output("-un");
    let group = current_id_output("-gn");

    for args in [
        argv(&[".", "-user", &user, "-group", &group]),
        argv(&[".", "-user", &uid.to_string(), "-group", &gid.to_string()]),
    ] {
        let plan = plan_command(parse_command(&args).unwrap(), 1).unwrap();
        let predicates = predicate_items(&plan.expr);

        assert!(predicates.iter().any(|predicate| matches!(
            predicate,
            RuntimePredicate::User(expected) if *expected == uid
        )));
        assert!(predicates.iter().any(|predicate| matches!(
            predicate,
            RuntimePredicate::Group(expected) if *expected == gid
        )));
    }
}

#[test]
fn lowers_nouser_and_nogroup_into_runtime_predicates() {
    let plan = plan_command(
        parse_command(&argv(&[".", "-nouser", "-nogroup"])).unwrap(),
        1,
    )
    .unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates
        .iter()
        .any(|predicate| matches!(predicate, RuntimePredicate::NoUser)));
    assert!(predicates
        .iter()
        .any(|predicate| matches!(predicate, RuntimePredicate::NoGroup)));
}

#[test]
fn rejects_unknown_user_and_group_names() {
    let user_error = plan_command(
        parse_command(&argv(&[".", "-user", "definitely_no_such_user_12345"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(user_error.message.contains("not the name of a known user"));

    let group_error = plan_command(
        parse_command(&argv(&[".", "-group", "definitely_no_such_group_12345"])).unwrap(),
        1,
    )
    .unwrap_err();
    assert!(group_error
        .message
        .contains("not the name of an existing group"));
}

fn predicate_items(expr: &RuntimeExpr) -> Vec<&RuntimePredicate> {
    match expr {
        RuntimeExpr::And(items) => items.iter().flat_map(predicate_items).collect(),
        RuntimeExpr::Predicate(predicate) => vec![predicate],
        RuntimeExpr::Or(left, right) => {
            let mut items = predicate_items(left);
            items.extend(predicate_items(right));
            items
        }
        RuntimeExpr::Not(inner) => predicate_items(inner),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => Vec::new(),
    }
}

fn current_id_output(flag: &str) -> String {
    let output = Command::new("id").arg(flag).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
