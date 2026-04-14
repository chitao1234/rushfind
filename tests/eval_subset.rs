use findoxide::entry::{EntryContext, EntryKind};
use findoxide::eval::evaluate;
use findoxide::output::RecordingSink;
use findoxide::planner::{OutputAction, RuntimeExpr, RuntimePredicate};
use std::path::PathBuf;

#[test]
fn matching_name_predicate_prints_the_entry_path() {
    let entry = EntryContext::synthetic(PathBuf::from("src/lib.rs"), EntryKind::File, 1);
    let expr = RuntimeExpr::And(vec![
        RuntimeExpr::Predicate(RuntimePredicate::Name {
            pattern: "*.rs".into(),
            case_insensitive: false,
        }),
        RuntimeExpr::Action(OutputAction::Print),
    ]);
    let mut sink = RecordingSink::default();

    let matched = evaluate(&expr, &entry, &mut sink).unwrap();

    assert!(matched);
    assert_eq!(sink.into_utf8(), "src/lib.rs\n");
}

#[test]
fn iname_predicate_is_case_insensitive() {
    let entry = EntryContext::synthetic(PathBuf::from("README.MD"), EntryKind::File, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Name {
        pattern: "*.md".into(),
        case_insensitive: true,
    });
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, &mut sink).unwrap());
}

#[test]
fn type_predicate_filters_by_entry_kind() {
    let entry = EntryContext::synthetic(PathBuf::from("src"), EntryKind::Directory, 0);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Type(
        findoxide::ast::FileTypeFilter::Directory,
    ));
    let mut sink = RecordingSink::default();

    assert!(evaluate(&expr, &entry, &mut sink).unwrap());
}
