mod support;

use findoxide::identity::FileIdentity;
use findoxide::numeric::NumericComparison;
use findoxide::parser::parse_command;
use findoxide::planner::{RuntimeExpr, RuntimePredicate, plan_command};
use std::fs;
use std::os::unix::fs as unix_fs;
use support::{argv, path_arg};
use tempfile::tempdir;

#[test]
fn lowers_inum_and_links_into_runtime_numeric_comparisons() {
    let ast = parse_command(&argv(&[".", "-inum", "+42", "-links", "-2"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();
    let predicates = predicate_items(&plan.expr);

    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Inum(NumericComparison::GreaterThan(42))
    )));
    assert!(predicates.iter().any(|predicate| matches!(
        predicate,
        RuntimePredicate::Links(NumericComparison::LessThan(2))
    )));
}

#[test]
fn samefile_reference_uses_follow_mode_rules() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("real.txt"), "hello\n").unwrap();
    unix_fs::symlink(root.path().join("real.txt"), root.path().join("ref-link")).unwrap();

    for flag in ["-P", "-H", "-L"] {
        let args = vec![
            flag.into(),
            ".".into(),
            "-samefile".into(),
            path_arg(&root.path().join("ref-link")),
        ];
        let plan = plan_command(parse_command(&args).unwrap(), 1).unwrap();
        let expected = match flag {
            "-P" => FileIdentity::from_metadata(
                &fs::symlink_metadata(root.path().join("ref-link")).unwrap(),
            ),
            "-H" | "-L" => {
                FileIdentity::from_metadata(&fs::metadata(root.path().join("ref-link")).unwrap())
            }
            _ => unreachable!(),
        };

        assert_eq!(samefile_identity(&plan.expr), expected);
    }
}

#[test]
fn broken_samefile_reference_falls_back_to_physical_metadata() {
    let root = tempdir().unwrap();
    unix_fs::symlink(root.path().join("missing"), root.path().join("broken-link")).unwrap();
    let expected =
        FileIdentity::from_metadata(&fs::symlink_metadata(root.path().join("broken-link")).unwrap());

    for flag in ["-H", "-L"] {
        let args = vec![
            flag.into(),
            ".".into(),
            "-samefile".into(),
            path_arg(&root.path().join("broken-link")),
        ];
        let plan = plan_command(parse_command(&args).unwrap(), 1).unwrap();

        assert_eq!(samefile_identity(&plan.expr), expected);
    }
}

#[test]
fn nonexistent_samefile_reference_is_a_planning_error() {
    let root = tempdir().unwrap();
    let args = vec![
        ".".into(),
        "-samefile".into(),
        path_arg(&root.path().join("missing-link")),
    ];
    let error = plan_command(parse_command(&args).unwrap(), 1).unwrap_err();

    assert!(error.message.contains("missing-link"));
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
        RuntimeExpr::Action(_) => Vec::new(),
    }
}

fn samefile_identity(expr: &RuntimeExpr) -> FileIdentity {
    predicate_items(expr)
        .into_iter()
        .find_map(|predicate| match predicate {
            RuntimePredicate::SameFile(identity) => Some(*identity),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected samefile predicate in {expr:?}"))
}
