mod support;

use rushfind::ast::{CommandAst, Expr, FileTypeFilter, GlobalOption, Predicate};
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
        Expr::Predicate(Predicate::XType(FileTypeFilter::Symlink))
    );
}
