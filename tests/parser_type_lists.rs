mod support;

use rushfind::ast::{
    CommandAst, CompatibilityOptions, Expr, FileTypeFilter, FileTypeMatcher, Predicate,
};
use rushfind::parser::parse_command;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_type_and_xtype_comma_lists() {
    let ast = parse_command(&argv(&[".", "-type", "f,d", "-xtype", "l,s"])).unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![PathBuf::from(".")],
            start_paths_explicit: true,
            compatibility_options: CompatibilityOptions::default(),
            global_options: vec![],
            expr: Expr::And(vec![
                Expr::Predicate(Predicate::Type(FileTypeMatcher::from_filters([
                    FileTypeFilter::File,
                    FileTypeFilter::Directory,
                ]))),
                Expr::Predicate(Predicate::XType(FileTypeMatcher::from_filters([
                    FileTypeFilter::Symlink,
                    FileTypeFilter::Socket,
                ]))),
            ]),
        }
    );
}

#[test]
fn parses_single_type_as_singleton_matcher() {
    let ast = parse_command(&argv(&[".", "-type", "f"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::Predicate(Predicate::Type(FileTypeMatcher::single(
            FileTypeFilter::File
        )))
    );
}

#[test]
fn rejects_malformed_type_lists() {
    for value in ["f,", ",f", "f,,d", "z", "f,z"] {
        let error = parse_command(&argv(&[".", "-type", value])).unwrap_err();
        assert!(
            error.message.contains("-type"),
            "{value} -> {}",
            error.message
        );
    }
}

#[test]
fn rejects_malformed_xtype_lists() {
    for value in ["l,", ",l", "l,,s", "z", "l,z"] {
        let error = parse_command(&argv(&[".", "-xtype", value])).unwrap_err();
        assert!(
            error.message.contains("-xtype"),
            "{value} -> {}",
            error.message
        );
    }
}

#[test]
fn parses_door_type_filters_for_gnu_compatibility() {
    let ast = parse_command(&argv(&[".", "-type", "D", "-xtype", "f,D"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Type(FileTypeMatcher::single(
                FileTypeFilter::Door,
            ))),
            Expr::Predicate(Predicate::XType(FileTypeMatcher::from_filters([
                FileTypeFilter::File,
                FileTypeFilter::Door,
            ]))),
        ])
    );
}
