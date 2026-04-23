mod support;

use rushfind::ast::{Action, CommandAst, Expr, GlobalOption};
use rushfind::follow::FollowMode;
use rushfind::parser::parse_command;
use support::argv;

#[test]
fn parses_version_aliases_as_leading_global_options() {
    let short = parse_command(&argv(&["-version", "."])).unwrap();
    assert_eq!(
        short,
        CommandAst {
            start_paths: vec![".".into()],
            global_options: vec![GlobalOption::Version],
            expr: Expr::Action(Action::Print),
        }
    );

    let long = parse_command(&argv(&["-L", "--version", "."])).unwrap();
    assert_eq!(
        long.global_options,
        vec![
            GlobalOption::Follow(FollowMode::Logical),
            GlobalOption::Version,
        ]
    );
}

#[test]
fn version_flags_remain_leading_only() {
    for raw in ["-version", "--version"] {
        let error = parse_command(&argv(&[".", raw])).unwrap_err();
        assert!(
            error
                .message
                .contains(&format!("unsupported token in parser subset `{raw}`")),
            "{raw}: {}",
            error.message
        );
    }
}
