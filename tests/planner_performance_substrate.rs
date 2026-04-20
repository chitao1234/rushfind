mod support;

use rushfind::birth::read_birth_time;
use rushfind::file_output::FileOutputTerminator;
use rushfind::parser::parse_command;
use rushfind::planner::{OutputAction, RuntimeAction, RuntimeExpr, RuntimePredicate, plan_command};
use std::fs;
use support::{argv, predicate_labels as collect_predicate_labels};
use tempfile::tempdir;

#[test]
fn predicate_reordering_cases_match_expected_order() {
    for (case, args, expected) in [
        (
            "pure read-only chain",
            &[
                ".", "-uid", "0", "-name", "*.rs", "-false", "-type", "f", "-perm", "644",
            ][..],
            &["false", "name", "type", "uid", "perm"][..],
        ),
        (
            "equal-cost predicates keep relative order",
            &[".", "-gid", "0", "-uid", "0", "-links", "+1", "-inum", "1"][..],
            &["gid", "uid", "links", "inum"][..],
        ),
        (
            "directory probe stays after cheaper type checks",
            &[".", "-empty", "-type", "f", "-name", "*.rs"][..],
            &["name", "type", "empty"][..],
        ),
        (
            "fstype is reorderable in read-only segments",
            &[".", "-uid", "0", "-fstype", "tmpfs", "-name", "*.rs"][..],
            &["name", "uid", "fstype"][..],
        ),
        (
            "access predicates are reorderable in read-only segments",
            &[".", "-uid", "0", "-readable", "-name", "*.rs"][..],
            &["name", "uid", "readable"][..],
        ),
        (
            "regex sorts after path and before metadata checks",
            &[".", "-uid", "0", "-regex", ".*\\.rs", "-path", "./src/*"][..],
            &["path", "regex", "uid"][..],
        ),
    ] {
        let ast = parse_command(&argv(args)).unwrap();
        let plan = plan_command(ast, 1).unwrap();
        assert_eq!(predicate_labels(&plan.expr), expected, "{case}");
    }

    let root = tempdir().unwrap();
    let reference = root.path().join("reference.txt");
    fs::write(&reference, "reference\n").unwrap();
    if read_birth_time(&reference, true).unwrap().is_none() {
        return;
    }

    let args = vec![
        ".".into(),
        "-newerBm".into(),
        reference.as_os_str().to_os_string(),
        "-size".into(),
        "+0c".into(),
        "-name".into(),
        "*.rs".into(),
    ];
    let ast = parse_command(&args).unwrap();
    let plan = plan_command(ast, 1).unwrap();
    assert_eq!(
        predicate_labels(&plan.expr),
        ["name", "size", "newer"],
        "birth time stays after active metadata checks"
    );
}

#[test]
fn optimizer_barrier_cases_preserve_expected_linear_order() {
    for (case, args, expected) in [
        (
            "actions block cross-boundary reordering",
            &[".", "-uid", "0", "-print", "-name", "*.rs"][..],
            &["uid", "print", "name"][..],
        ),
        (
            "traversal controls are optimizer barriers",
            &[".", "-uid", "0", "-maxdepth", "1", "-name", "*.rs"][..],
            &["uid", "barrier", "name", "print"][..],
        ),
        (
            "prune is an optimizer barrier",
            &[".", "-uid", "0", "-prune", "-name", "*.rs"][..],
            &["uid", "prune", "name", "print"][..],
        ),
        (
            "daystart is an optimizer barrier",
            &[
                ".",
                "-uid",
                "0",
                "-daystart",
                "-mtime",
                "0",
                "-name",
                "*.rs",
            ][..],
            &["uid", "barrier", "name", "mtime", "print"][..],
        ),
        (
            "exec actions are optimizer barriers",
            &[
                ".", "-name", "*.rs", "-exec", "echo", "{}", ";", "-uid", "0",
            ][..],
            &["name", "exec:semicolon", "uid"][..],
        ),
        (
            "depth and delete are optimizer barriers",
            &[".", "-uid", "0", "-depth", "-name", "*.tmp", "-delete"][..],
            &["uid", "barrier", "name", "delete"][..],
        ),
    ] {
        let ast = parse_command(&argv(args)).unwrap();
        let plan = plan_command(ast, 1).unwrap();
        assert_eq!(linear_labels(&plan.expr), expected, "{case}");
    }
}

#[test]
fn or_stays_a_boundary_and_not_optimizes_its_child_subtree() {
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
    assert_eq!(not_inner_labels(&plan.expr), vec!["name", "uid"]);
}

fn predicate_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    collect_predicate_labels(expr, |predicate| Some(predicate_label(predicate)))
}

fn linear_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let mut labels = Vec::new();
    collect_linear_labels(expr, &mut labels);
    labels
}

fn collect_linear_labels(expr: &RuntimeExpr, labels: &mut Vec<&'static str>) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items.iter() {
                collect_linear_labels(item, labels);
            }
        }
        other => labels.push(expr_label(other)),
    }
}

fn not_inner_labels(expr: &RuntimeExpr) -> Vec<&'static str> {
    let inner = find_not_inner(expr).expect("expected a Not expression");
    let RuntimeExpr::And(inner_items) = inner else {
        panic!("expected Not inner to be And, got {inner:?}");
    };

    inner_items.iter().map(expr_label).collect()
}

fn find_not_inner(expr: &RuntimeExpr) -> Option<&RuntimeExpr> {
    match expr {
        RuntimeExpr::Not(inner) => Some(inner.as_ref()),
        RuntimeExpr::And(items) => items.iter().find_map(find_not_inner),
        RuntimeExpr::Or(left, right) => find_not_inner(left).or_else(|| find_not_inner(right)),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Action(_) | RuntimeExpr::Barrier => None,
    }
}

fn expr_label(expr: &RuntimeExpr) -> &'static str {
    match expr {
        RuntimeExpr::Predicate(predicate) => predicate_label(predicate),
        RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)) => "print",
        RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print0)) => "print0",
        RuntimeExpr::Action(RuntimeAction::Printf(_)) => "printf",
        RuntimeExpr::Action(RuntimeAction::FilePrint { terminator, .. }) => match terminator {
            FileOutputTerminator::Newline => "fprint",
            FileOutputTerminator::Nul => "fprint0",
        },
        RuntimeExpr::Action(RuntimeAction::FilePrintf { .. }) => "fprintf",
        RuntimeExpr::Action(RuntimeAction::Ls) => "ls",
        RuntimeExpr::Action(RuntimeAction::FileLs { .. }) => "fls",
        RuntimeExpr::Action(RuntimeAction::Quit) => "quit",
        RuntimeExpr::Action(RuntimeAction::Delete) => "delete",
        RuntimeExpr::Action(RuntimeAction::ExecImmediate(_)) => "exec:semicolon",
        RuntimeExpr::Action(RuntimeAction::ExecBatched(_)) => "exec:batch",
        RuntimeExpr::Action(RuntimeAction::ExecPrompt(spec)) => match spec.semantics {
            rushfind::exec::ExecSemantics::Normal => "ok:semicolon",
            rushfind::exec::ExecSemantics::DirLocal => "okdir:semicolon",
        },
        RuntimeExpr::And(_) => "and",
        RuntimeExpr::Or(_, _) => "or",
        RuntimeExpr::Not(_) => "not",
        RuntimeExpr::Barrier => "barrier",
    }
}

fn predicate_label(predicate: &RuntimePredicate) -> &'static str {
    match predicate {
        RuntimePredicate::Readable => "readable",
        RuntimePredicate::Writable => "writable",
        RuntimePredicate::Executable => "executable",
        RuntimePredicate::FsType(_) => "fstype",
        RuntimePredicate::Prune => "prune",
        RuntimePredicate::Name { .. } => "name",
        RuntimePredicate::Path { .. } => "path",
        RuntimePredicate::Regex(_) => "regex",
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
            (rushfind::time::TimestampKind::Access, rushfind::time::RelativeTimeUnit::Days) => {
                "atime"
            }
            (rushfind::time::TimestampKind::Birth, _) => "time-birth",
            (rushfind::time::TimestampKind::Change, rushfind::time::RelativeTimeUnit::Days) => {
                "ctime"
            }
            (
                rushfind::time::TimestampKind::Modification,
                rushfind::time::RelativeTimeUnit::Days,
            ) => "mtime",
            (rushfind::time::TimestampKind::Access, rushfind::time::RelativeTimeUnit::Minutes) => {
                "amin"
            }
            (rushfind::time::TimestampKind::Change, rushfind::time::RelativeTimeUnit::Minutes) => {
                "cmin"
            }
            (
                rushfind::time::TimestampKind::Modification,
                rushfind::time::RelativeTimeUnit::Minutes,
            ) => "mmin",
        },
        RuntimePredicate::Type(_) => "type",
        RuntimePredicate::XType(_) => "xtype",
        RuntimePredicate::True => "true",
        RuntimePredicate::False => "false",
    }
}
