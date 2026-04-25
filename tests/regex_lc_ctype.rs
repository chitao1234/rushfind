use rushfind::ctype::{CtypeProfile, resolve_ctype_profile_from};
use rushfind::regex_match::{RegexDialect, RegexMatcher};
use std::ffi::OsStr;

fn utf8() -> CtypeProfile {
    resolve_ctype_profile_from(vec![("LC_CTYPE", "en_US.UTF-8")])
}

#[test]
fn utf8_gnu_regex_dot_matches_one_multibyte_character() {
    let ctype = utf8();
    let matcher = RegexMatcher::compile_with_ctype(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(".*/x."),
        false,
        &ctype,
    )
    .unwrap();

    assert!(
        matcher
            .is_match_with_ctype(OsStr::new("./xé"), &ctype)
            .unwrap()
    );
}

#[test]
fn utf8_gnu_regex_posix_alpha_matches_non_ascii_letter() {
    let ctype = utf8();
    let matcher = RegexMatcher::compile_with_ctype(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(".*/[[:alpha:]]"),
        false,
        &ctype,
    )
    .unwrap();

    assert!(
        matcher
            .is_match_with_ctype(OsStr::new("./é"), &ctype)
            .unwrap()
    );
    assert!(
        !matcher
            .is_match_with_ctype(OsStr::new("./5"), &ctype)
            .unwrap()
    );
}

#[test]
fn utf8_gnu_iregex_uses_single_character_case_folding() {
    let ctype = utf8();
    let matcher = RegexMatcher::compile_with_ctype(
        "-iregex",
        RegexDialect::PosixExtended,
        OsStr::new(".*/á"),
        true,
        &ctype,
    )
    .unwrap();

    assert!(
        matcher
            .is_match_with_ctype(OsStr::new("./Á"), &ctype)
            .unwrap()
    );
    assert!(
        !matcher
            .is_match_with_ctype(OsStr::new("./ss"), &ctype)
            .unwrap()
    );
}

#[cfg(unix)]
#[test]
fn utf8_gnu_regex_dot_does_not_classify_invalid_byte_as_alpha() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let ctype = utf8();
    let matcher = RegexMatcher::compile_with_ctype(
        "-regex",
        RegexDialect::PosixExtended,
        OsStr::new(".*/x[[:alpha:]]"),
        false,
        &ctype,
    )
    .unwrap();
    let candidate = OsString::from_vec(b"./x\xc3".to_vec());

    assert!(
        !matcher
            .is_match_with_ctype(candidate.as_os_str(), &ctype)
            .unwrap()
    );
}

#[test]
fn utf8_gnu_regex_word_escape_remains_ascii_word_for_now() {
    let ctype = utf8();
    let matcher = RegexMatcher::compile_with_ctype(
        "-regex",
        RegexDialect::Emacs,
        OsStr::new(r".*/\w"),
        false,
        &ctype,
    )
    .unwrap();

    assert!(
        matcher
            .is_match_with_ctype(OsStr::new("./A"), &ctype)
            .unwrap()
    );
    assert!(
        !matcher
            .is_match_with_ctype(OsStr::new("./é"), &ctype)
            .unwrap()
    );
}
