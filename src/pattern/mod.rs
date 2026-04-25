mod ir;
mod owned;
mod parse;

use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::sync::Arc;

pub fn matches_pattern(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    CompiledGlob::compile(
        if pathname {
            if case_insensitive { "-ipath" } else { "-path" }
        } else if case_insensitive {
            "-iname"
        } else {
            "-name"
        },
        pattern,
        if case_insensitive {
            GlobCaseMode::Insensitive
        } else {
            GlobCaseMode::Sensitive
        },
        if pathname {
            GlobSlashMode::Pathname
        } else {
            GlobSlashMode::Literal
        },
    )
    .and_then(|glob| glob.is_match(candidate))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobCaseMode {
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobSlashMode {
    Literal,
    Pathname,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledGlobInner {
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    program: ir::GlobProgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGlob {
    inner: Arc<CompiledGlobInner>,
}

impl CompiledGlob {
    pub fn compile(
        flag: &'static str,
        pattern: &OsStr,
        case_mode: GlobCaseMode,
        slash_mode: GlobSlashMode,
    ) -> Result<Self, Diagnostic> {
        let original_pattern = pattern.as_encoded_bytes().to_vec();
        let parsed = parse::compile_pattern(flag, &original_pattern, case_mode, slash_mode)?;
        Ok(Self {
            inner: Arc::new(CompiledGlobInner {
                case_mode,
                slash_mode,
                program: parsed.program,
            }),
        })
    }

    pub fn is_match(&self, candidate: &OsStr) -> Result<bool, Diagnostic> {
        owned::matches(
            &self.inner.program,
            self.inner.case_mode,
            self.inner.slash_mode,
            candidate.as_encoded_bytes(),
        )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{CompiledGlob, GlobCaseMode, GlobSlashMode, matches_pattern};
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn matches_pattern_uses_owned_case_insensitive_semantics() {
        let matched =
            matches_pattern(OsStr::new("*.rs"), OsStr::new("MAIN.RS"), true, true).unwrap();

        assert!(matched);
    }

    #[test]
    fn basename_star_does_not_treat_slash_as_special() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("*a/b*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("xa/by")).unwrap());
    }

    #[test]
    fn path_star_can_cross_slashes_like_gnu_find() {
        let glob = CompiledGlob::compile(
            "-path",
            OsStr::new("./src/*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("./src/lib.rs")).unwrap());
        assert!(glob.is_match(OsStr::new("./src/nested/lib.rs")).unwrap());
    }

    #[test]
    fn c_locale_case_insensitive_matching_is_ascii_only() {
        let glob = CompiledGlob::compile(
            "-iname",
            OsStr::new("résumé.*"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("résumé.MD")).unwrap());
        assert!(!glob.is_match(OsStr::new("RÉSUMÉ.MD")).unwrap());
    }

    #[test]
    fn bracket_ranges_follow_c_locale_byte_order() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-C]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("Bravo")).unwrap());
        assert!(!glob.is_match(OsStr::new("delta")).unwrap());
    }

    #[test]
    fn non_utf8_candidates_are_matched_without_lossy_conversion() {
        let candidate = OsString::from_vec(vec![b'f', b'o', b'o', 0xff]);
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("foo*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(candidate.as_os_str()).unwrap());
    }

    #[test]
    fn case_insensitive_patterns_match_directly() {
        let glob = CompiledGlob::compile(
            "-iname",
            OsStr::new("*.md"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("README.MD")).unwrap());
    }

    #[test]
    fn bracket_patterns_match_directly() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-Z]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.is_match(OsStr::new("Alpha")).unwrap());
    }

    #[test]
    fn byte_c_glob_supports_posix_character_classes() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[[:alpha:]][[:digit:]]"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();

        assert!(glob.is_match(OsStr::new("A5")).unwrap());
        assert!(glob.is_match(OsStr::new("z9")).unwrap());
        assert!(!glob.is_match(OsStr::new("é5")).unwrap());
    }

    #[test]
    fn byte_c_glob_rejects_unknown_posix_class() {
        let error = CompiledGlob::compile(
            "-name",
            OsStr::new("[[:emoji:]]"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap_err();

        assert!(error.message.contains("unsupported POSIX character class"));
    }
}
