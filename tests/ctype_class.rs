use rushfind::ctype::case::{chars_equal_folded, fold_char};
use rushfind::ctype::class::{PosixClass, class_contains};

#[test]
fn digit_and_xdigit_are_ascii_scoped() {
    assert!(class_contains(PosixClass::Digit, '5'));
    assert!(!class_contains(PosixClass::Digit, '٣'));
    assert!(class_contains(PosixClass::XDigit, 'f'));
    assert!(class_contains(PosixClass::XDigit, 'F'));
    assert!(!class_contains(PosixClass::XDigit, 'ｆ'));
}

#[test]
fn alpha_and_case_classes_include_unicode_letters() {
    assert!(class_contains(PosixClass::Alpha, 'é'));
    assert!(class_contains(PosixClass::Lower, 'é'));
    assert!(class_contains(PosixClass::Upper, 'É'));
    assert!(class_contains(PosixClass::Alnum, 'é'));
    assert!(!class_contains(PosixClass::Alnum, '-'));
}

#[test]
fn single_character_case_folding_matches_non_ascii_pairs() {
    assert!(chars_equal_folded('Á', 'á'));
    assert_eq!(fold_char('A'), 'a');
    assert_eq!(fold_char('É'), 'é');
}

#[test]
fn single_character_case_folding_does_not_expand_sharp_s() {
    assert_eq!(fold_char('ß'), 'ß');
    assert!(!chars_equal_folded('ß', 's'));
}
