mod support;

use rushfind::ast::{
    CommandAst, CompatibilityOptions, Expr, FileTypeFilter, FileTypeMatcher, GlobalOption,
    Predicate,
};
use rushfind::follow::FollowMode;
use rushfind::parser::parse_command;
use support::argv;

#[test]
fn parses_global_follow_mode_options_before_paths() {
    let ast = parse_command(&argv(&["-L", ".", "-name", "*.rs"])).unwrap();

    assert_eq!(
        ast,
        CommandAst {
            start_paths: vec![".".into()],
            start_paths_explicit: true,
            compatibility_options: CompatibilityOptions::default(),
            global_options: vec![GlobalOption::Follow(FollowMode::Logical)],
            expr: Expr::Predicate(Predicate::Name {
                pattern: "*.rs".into(),
                case_insensitive: false,
            }),
        }
    );
}

#[test]
fn parses_last_follow_mode_when_multiple_are_present() {
    let ast = parse_command(&argv(&["-P", "-H", ".", "-print"])).unwrap();

    assert_eq!(
        ast.global_options,
        vec![
            GlobalOption::Follow(FollowMode::Physical),
            GlobalOption::Follow(FollowMode::CommandLineOnly),
        ]
    );
}

#[test]
fn parses_xtype_as_a_normal_predicate() {
    let ast = parse_command(&argv(&[".", "-xtype", "l"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::Predicate(Predicate::XType(FileTypeMatcher::single(
            FileTypeFilter::Symlink
        )))
    );
}

#[test]
fn parses_positional_follow_as_compatibility_option() {
    let ast = parse_command(&argv(&[".", "-follow", "-print"])).unwrap();

    assert!(ast.compatibility_options.follow);
    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Compatibility(
                rushfind::ast::CompatibilityPredicate::Follow,
            )),
            Expr::Action(rushfind::ast::Action::Print),
        ])
    );
}

#[test]
fn positional_follow_is_not_a_leading_normal_option() {
    let error = parse_command(&argv(&["-follow", ".", "-print"])).unwrap_err();

    assert!(
        error.message.contains("unexpected") || error.message.contains("unsupported"),
        "{}",
        error.message
    );
}

#[test]
fn parses_context_as_a_recognized_predicate() {
    let ast = parse_command(&argv(&[".", "-context", "system_u:object_r:tmp_t:s0"])).unwrap();

    assert_eq!(
        ast.expr,
        Expr::Predicate(Predicate::Context("system_u:object_r:tmp_t:s0".into()))
    );
}

#[test]
fn rejects_missing_context_operand() {
    let error = parse_command(&argv(&[".", "-context"])).unwrap_err();

    assert!(error.message.contains("-context"), "{}", error.message);
}
