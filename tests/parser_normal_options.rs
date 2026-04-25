mod support;

use rushfind::ast::{
    Action, CompatibilityOptions, CompatibilityPredicate, DebugOption, Expr, Files0From,
    GlobalOption, Predicate, WarningMode,
};
use rushfind::parser::parse_command;
use std::ffi::OsString;
use std::path::PathBuf;
use support::argv;

#[test]
fn parses_help_and_leading_debug_and_optimizer_options() {
    let ast = parse_command(&argv(&["--help", "-O3", "-D", "search,stat,unknown"])).unwrap();

    assert_eq!(ast.start_paths, vec![PathBuf::from(".")]);
    assert!(!ast.start_paths_explicit);
    assert_eq!(ast.global_options, vec![GlobalOption::Help]);
    assert_eq!(ast.compatibility_options.optimizer_level, Some(3));
    assert_eq!(
        ast.compatibility_options.debug_options,
        vec![DebugOption::Search, DebugOption::Stat]
    );
    assert_eq!(
        ast.compatibility_options.unknown_debug_options,
        vec![OsString::from("unknown")]
    );
    assert_eq!(ast.expr, Expr::Action(Action::Print));
}

#[test]
fn parses_expression_position_compatibility_options_as_true_atoms() {
    let ast = parse_command(&argv(&[
        "-files0-from",
        "roots.bin",
        "-noleaf",
        "-nowarn",
        "-warn",
        "-ignore_readdir_race",
        "-noignore_readdir_race",
        "-print",
    ]))
    .unwrap();

    assert_eq!(ast.start_paths, vec![PathBuf::from(".")]);
    assert!(!ast.start_paths_explicit);
    assert_eq!(
        ast.compatibility_options.files0_from,
        Some(Files0From::Path(PathBuf::from("roots.bin")))
    );
    assert_eq!(ast.compatibility_options.warning_mode, WarningMode::Warn);
    assert!(ast.compatibility_options.noleaf);
    assert_eq!(ast.compatibility_options.ignore_readdir_race, Some(false));

    assert_eq!(
        ast.expr,
        Expr::And(vec![
            Expr::Predicate(Predicate::Compatibility(CompatibilityPredicate::Files0From,)),
            Expr::Predicate(Predicate::Compatibility(CompatibilityPredicate::NoLeaf)),
            Expr::Predicate(Predicate::Compatibility(CompatibilityPredicate::NoWarn)),
            Expr::Predicate(Predicate::Compatibility(CompatibilityPredicate::Warn)),
            Expr::Predicate(Predicate::Compatibility(
                CompatibilityPredicate::IgnoreReaddirRace,
            )),
            Expr::Predicate(Predicate::Compatibility(
                CompatibilityPredicate::NoIgnoreReaddirRace,
            )),
            Expr::Action(Action::Print),
        ])
    );
}

#[test]
fn tracks_explicit_start_paths_separately_from_default_path() {
    let defaulted = parse_command(&argv(&["-name", "*.rs"])).unwrap();
    assert_eq!(defaulted.start_paths, vec![PathBuf::from(".")]);
    assert!(!defaulted.start_paths_explicit);

    let explicit = parse_command(&argv(&["src", "-name", "*.rs"])).unwrap();
    assert_eq!(explicit.start_paths, vec![PathBuf::from("src")]);
    assert!(explicit.start_paths_explicit);
}

#[test]
fn parses_files0_from_stdin_source() {
    let ast = parse_command(&argv(&["-files0-from", "-", "-print"])).unwrap();

    assert_eq!(
        ast.compatibility_options.files0_from,
        Some(Files0From::Stdin)
    );
}

#[test]
fn rejects_invalid_optimizer_options() {
    for args in [vec!["-O"], vec!["-O", "3"], vec!["-Ofoo"], vec!["-O-1"]] {
        let error = parse_command(&argv(&args)).unwrap_err();
        assert!(
            error.message.contains("-O"),
            "{:?} -> {}",
            args,
            error.message
        );
    }
}

#[test]
fn rejects_missing_debug_and_files0_from_operands() {
    for args in [vec!["-D"], vec!["-files0-from"]] {
        let error = parse_command(&argv(&args)).unwrap_err();
        assert!(
            error.message.contains(args[0]),
            "{:?} -> {}",
            args,
            error.message
        );
    }
}

#[test]
fn keeps_d_and_o_positional_after_explicit_paths() {
    for args in [vec![".", "-D", "search"], vec![".", "-O3"]] {
        let error = parse_command(&argv(&args)).unwrap_err();
        assert!(
            error.message.contains("unknown")
                || error.message.contains("expected")
                || error.message.contains("unsupported"),
            "{:?} -> {}",
            args,
            error.message
        );
    }
}

#[test]
fn compatibility_options_default_to_empty_state() {
    let ast = parse_command(&argv(&[".", "-print"])).unwrap();

    assert_eq!(ast.compatibility_options, CompatibilityOptions::default());
}
