mod support;

use findoxide::parser::parse_command;
use findoxide::planner::{OutputAction, RuntimeExpr, RuntimePredicate, plan_command};
use support::argv;

#[test]
fn pure_read_only_and_chain_reorders_to_stable_cheap_first_order() {
    let ast = parse_command(&argv(&[
        ".",
        "-uid",
        "0",
        "-name",
        "*.rs",
        "-false",
        "-type",
        "f",
        "-perm",
        "644",
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
        ".",
        "-gid",
        "0",
        "-uid",
        "0",
        "-links",
        "+1",
        "-inum",
        "1",
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
        ".",
        "(",
        "-uid",
        "0",
        "-o",
        "-name",
        "*.rs",
        ")",
        "-type",
        "f",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(linear_labels(&plan.expr), vec!["or", "type", "print"]);

    let ast = parse_command(&argv(&[
        ".",
        "!",
        "(",
        "-uid",
        "0",
        "-name",
        "*.rs",
        ")",
        "-type",
        "f",
    ]))
    .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(linear_labels(&plan.expr), vec!["not", "type", "print"]);
    assert_eq!(not_inner_labels(&plan.expr), vec!["uid", "name"]);
}

#[test]
fn traversal_controls_are_optimizer_barriers() {
    let ast = parse_command(&argv(&[".", "-uid", "0", "-maxdepth", "1", "-name", "*.rs"]))
        .unwrap();
    let plan = plan_command(ast, 1).unwrap();

    assert_eq!(
        linear_labels(&plan.expr),
        vec!["uid", "traversal", "name", "print"]
    );
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
        RuntimeExpr::Action(_) | RuntimeExpr::TraversalBoundary => {}
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
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::TraversalBoundary => {
            None
        }
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
        RuntimeExpr::TraversalBoundary => "traversal",
    }
}

fn predicate_label(predicate: &RuntimePredicate) -> &'static str {
    match predicate {
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
        RuntimePredicate::Type(_) => "type",
        RuntimePredicate::XType(_) => "xtype",
        RuntimePredicate::True => "true",
        RuntimePredicate::False => "false",
    }
}
