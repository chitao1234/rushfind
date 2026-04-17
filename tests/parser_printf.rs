mod support;

use findoxide::parser::parse_command;
use support::argv;

#[test]
fn parses_printf_with_a_single_format_argument() {
    let ast = parse_command(&argv(&[".", "-printf", "%p\\n"])).unwrap();
    assert_eq!(ast.start_paths.len(), 1);
}

#[test]
fn reports_missing_printf_argument() {
    let error = parse_command(&argv(&[".", "-printf"])).unwrap_err();
    assert!(error.message.contains("missing argument for `-printf`"));
}
