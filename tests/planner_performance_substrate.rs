mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{OutputAction, RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn pure_read_only_and_chain_reorders_to_stable_cheap_first_order() {
    let ast = parse_command(&argv(&[
        ".", "-uid", "0", "-name", "*.rs", "-false", "-type", "f", "-perm", "644",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        predicate_labels(&plan.expr),
        vec!["false", "name", "type", "uid", "perm"]
    );
}

#[test]
fn equal_cost_predicates_keep_original_relative_order() {
    let ast = parse_command(&argv(&[
        ".", "-gid", "0", "-uid", "0", "-links", "+1", "-inum", "1",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        predicate_labels(&plan.expr),
        vec!["gid", "uid", "links", "inum"]
    );
}

#[test]
fn actions_block_cross_boundary_reordering() {
    let ast = parse_command(&argv(&[".", "-uid", "0", "-print", "-name", "*.rs"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(linear_labels(&plan.expr), vec!["uid", "print", "name"]);
}

#[test]
fn or_and_not_boundaries_are_not_crossed() {
    let ast = parse_command(&argv(&[
        ".", "(", "-uid", "0", "-o", "-name", "*.rs", ")", "-type", "f",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(linear_labels(&plan.expr), vec!["or", "type", "print"]);

    let ast = parse_command(&argv(&[
        ".", "!", "(", "-uid", "0", "-name", "*.rs", ")", "-type", "f",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(linear_labels(&plan.expr), vec!["not", "type", "print"]);
    assert_eq!(not_inner_labels(&plan.expr), vec!["uid", "name"]);
}

#[test]
fn traversal_controls_are_optimizer_barriers() {
    let ast = parse_command(&argv(&[
        ".",
        "-uid",
        "0",
        "-maxdepth",
        "1",
        "-name",
        "*.rs",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        linear_labels(&plan.expr),
        vec!["uid", "barrier", "name", "print"]
    );
}

#[test]
fn prune_is_an_optimizer_barrier() {
    let ast = parse_command(&argv(&[".", "-uid", "0", "-prune", "-name", "*.rs"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        linear_labels(&plan.expr),
        vec!["uid", "prune", "name", "print"]
    );
}

#[test]
fn daystart_is_an_optimizer_barrier() {
    let ast = parse_command(&argv(&[
        ".",
        "-uid",
        "0",
        "-daystart",
        "-mtime",
        "0",
        "-name",
        "*.rs",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        linear_labels(&plan.expr),
        vec!["uid", "barrier", "name", "mtime", "print"]
    );
}

#[test]
fn directory_probe_predicates_stay_after_cheaper_type_checks() {
    let ast = parse_command(&argv(&[".", "-empty", "-type", "f", "-name", "*.rs"])).unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(predicate_labels(&plan.expr), vec!["name", "type", "empty"]);
}

#[test]
fn birth_time_predicates_stay_after_ordinary_active_metadata_checks() {
    let ast = parse_command(&argv(&[
        ".",
        "-newerBm",
        "/proc/self/stat",
        "-size",
        "+0c",
        "-name",
        "*.rs",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(predicate_labels(&plan.expr), vec!["name", "size", "newer"]);
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
        RuntimeExpr::Predicate(predicate) => labels.push(predicate_label(predicate)),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => {}
    }
}

fn linear_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect_linear_labels(expr, &mut labels);
    labels
}

fn collect_linear_labels(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                collect_linear_labels(item, labels);
            }
        }
        other => labels.push(expr_label(other)),
    }
}

fn not_inner_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let inner = find_not_inner(expr).expect("expected a Not expression");
    let RuntimeExpr::And(inner_items) = inner.as_ref() else {
        panic!("expected Not inner to be And, got {inner:?}");
    };

    inner_items.iter().map(expr_label).collect()
}

fn find_not_inner(expr: &RuntimeExpr) -> Option<&Box<RuntimeExpr>> {
    match expr {
        RuntimeExpr::Not(inner) => Some(inner),
        RuntimeExpr::And(items) => items.iter().find_map(find_not_inner),
        RuntimeExpr::Or(left, right) => find_not_inner(left).or_else(|| find_not_inner(right)),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => None,
    }
}

fn expr_label(expr: &RuntimeExpr) -> &'static str {
    match expr {
        RuntimeExpr::Predicate(predicate) => predicate_label(predicate),
        RuntimeExpr::Action(OutputAction::Print) => "print",
        RuntimeExpr::Action(OutputAction::Print0) => "print0",
        RuntimeExpr::And(_) => "and",
        RuntimeExpr::Or(_, _) => "or",
        RuntimeExpr::Not(_) => "not",
        RuntimeExpr::Barrier => "barrier",
    }
}

fn predicate_label(predicate: &RuntimePredicate) -> &'static str {
    match predicate {
        RuntimePredicate::Prune => "prune",
        RuntimePredicate::Name { .. } => "name",
        RuntimePredicate::Path { .. } => "path",
        RuntimePredicate::Inum(_) => "inum",
        RuntimePredicate::Links(_) => "links",
        RuntimePredicate::SameFile(_) => "samefile",
        RuntimePredicate::LName { .. } => "lname",
        RuntimePredicate::Uid(_) => "uid",
        RuntimePredicate::Gid(_) => "gid",
        RuntimePredicate::User(_) => "user",
        RuntimePredicate::Group(_) => "group",
        RuntimePredicate::NoUser => "nouser",
        RuntimePredicate::NoGroup => "nogroup",
        RuntimePredicate::Perm(_) => "perm",
        RuntimePredicate::Size(_) => "size",
        RuntimePredicate::Empty => "empty",
        RuntimePredicate::Used(_) => "used",
        RuntimePredicate::Newer(_) => "newer",
        RuntimePredicate::RelativeTime(matcher) => match (matcher.kind, matcher.unit) {
            (findoxide::time::TimestampKind::Access, findoxide::time::RelativeTimeUnit::Days) => {
                "atime"
            }
            (findoxide::time::TimestampKind::Birth, _) => "time-birth",
            (findoxide::time::TimestampKind::Change, findoxide::time::RelativeTimeUnit::Days) => {
                "ctime"
            }
            (
                findoxide::time::TimestampKind::Modification,
                findoxide::time::RelativeTimeUnit::Days,
            ) => "mtime",
            (
                findoxide::time::TimestampKind::Access,
                findoxide::time::RelativeTimeUnit::Minutes,
            ) => "amin",
            (
                findoxide::time::TimestampKind::Change,
                findoxide::time::RelativeTimeUnit::Minutes,
            ) => "cmin",
            (
                findoxide::time::TimestampKind::Modification,
                findoxide::time::RelativeTimeUnit::Minutes,
            ) => "mmin",
        },
        RuntimePredicate::Type(_) => "type",
        RuntimePredicate::XType(_) => "xtype",
        RuntimePredicate::True => "true",
        RuntimePredicate::False => "false",
    }
}
