use rushfind::ctype::{CtypeProfile, resolve_ctype_profile_from};
use rushfind::pattern::{CompiledGlob, GlobCaseMode, GlobSlashMode};
use std::ffi::OsStr;

fn utf8() -> CtypeProfile {
    resolve_ctype_profile_from(vec![("LC_CTYPE", "en_US.UTF-8")])
}

#[test]
fn utf8_glob_question_matches_one_multibyte_character() {
    let ctype = utf8();
    let glob = CompiledGlob::compile_with_ctype(
        "-name",
        OsStr::new("x?"),
        GlobCaseMode::Sensitive,
        GlobSlashMode::Literal,
        &ctype,
    )
    .unwrap();

    assert!(glob.is_match_with_ctype(OsStr::new("xé"), &ctype).unwrap());
}

#[test]
fn utf8_glob_posix_alpha_matches_non_ascii_letter() {
    let ctype = utf8();
    let glob = CompiledGlob::compile_with_ctype(
        "-name",
        OsStr::new("[[:alpha:]]"),
        GlobCaseMode::Sensitive,
        GlobSlashMode::Literal,
        &ctype,
    )
    .unwrap();

    assert!(glob.is_match_with_ctype(OsStr::new("é"), &ctype).unwrap());
    assert!(!glob.is_match_with_ctype(OsStr::new("5"), &ctype).unwrap());
}

#[test]
fn utf8_glob_case_insensitive_uses_single_character_fold() {
    let ctype = utf8();
    let glob = CompiledGlob::compile_with_ctype(
        "-iname",
        OsStr::new("á"),
        GlobCaseMode::Insensitive,
        GlobSlashMode::Literal,
        &ctype,
    )
    .unwrap();

    assert!(glob.is_match_with_ctype(OsStr::new("Á"), &ctype).unwrap());
    assert!(!glob.is_match_with_ctype(OsStr::new("ss"), &ctype).unwrap());
}
