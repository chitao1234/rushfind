mod support;

use rushfind::ast::{GlobalOption, Predicate};
use rushfind::follow::FollowMode;
use rushfind::parser::parse_command;
use support::argv;

#[test]
fn parses_bsd_leading_aliases() {
    let ast = parse_command(&argv(&["-E", "-d", "-x", "-h", ".", "-regex", "foo"])).unwrap();

    assert_eq!(
        ast.global_options,
        vec![GlobalOption::Follow(FollowMode::CommandLineOnly),]
    );
    assert!(ast.compatibility_options.regex_extended);
    assert!(ast.compatibility_options.depth);
    assert!(ast.compatibility_options.same_file_system);
}

#[test]
fn parses_bsd_f_paths_before_expression_paths() {
    let ast = parse_command(&argv(&["-f", "-odd", "-f", "root", "-true"])).unwrap();

    assert!(ast.start_paths_explicit);
    assert_eq!(
        ast.start_paths,
        vec![
            std::path::PathBuf::from("-odd"),
            std::path::PathBuf::from("root")
        ]
    );
    assert!(matches!(
        ast.expr,
        rushfind::ast::Expr::Predicate(Predicate::True)
    ));
}
