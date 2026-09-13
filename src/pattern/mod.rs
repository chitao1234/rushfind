mod encoded;
mod ir;
mod owned;
mod parse;

use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::sync::Arc;

/// Compiles a pattern and matches one candidate against it.
///
/// `pathname` selects pathname-mode matching, where wildcards do not match `/`.
/// No predicate uses it: `-name` and `-path` both take literal slash handling,
/// because GNU `find` lets `*` cross `/` in `-path` as well.
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
    original_pattern: Vec<u8>,
    program: ir::GlobProgram,
    encoded_program: Option<encoded::EncodedGlobProgram>,
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
        let encoded_program = None;
        Ok(Self {
            inner: Arc::new(CompiledGlobInner {
                case_mode,
                slash_mode,
                original_pattern,
                program: parsed.program,
                encoded_program,
            }),
        })
    }

    pub fn compile_with_ctype(
        flag: &'static str,
        pattern: &OsStr,
        case_mode: GlobCaseMode,
        slash_mode: GlobSlashMode,
        ctype: &crate::ctype::CtypeProfile,
    ) -> Result<Self, Diagnostic> {
        let original_pattern = pattern.as_encoded_bytes().to_vec();
        let parsed = parse::compile_pattern(flag, &original_pattern, case_mode, slash_mode)?;
        let encoded_program = if ctype.is_byte_c()
            || ctype.is_unknown()
            || !crate::ctype::text::decodes_without_errors(ctype, &original_pattern)
        {
            None
        } else {
            Some(encoded::compile_pattern(flag, &original_pattern, ctype)?)
        };
        Ok(Self {
            inner: Arc::new(CompiledGlobInner {
                case_mode,
                slash_mode,
                original_pattern,
                program: parsed.program,
                encoded_program,
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

    pub fn is_match_with_ctype(
        &self,
        candidate: &OsStr,
        ctype: &crate::ctype::CtypeProfile,
    ) -> Result<bool, Diagnostic> {
        if ctype.is_byte_c() || ctype.is_unknown() {
            return self.is_match(candidate);
        }
        let Some(encoded_program) = &self.inner.encoded_program else {
            return self.is_match(candidate);
        };

        // Two ASCII sides decode to one unit per byte, so the byte matcher
        // answers it already. The pattern must be ASCII too: a non-ASCII
        // character can fold onto an ASCII one.
        if encoded_program.is_ascii_only() && candidate.as_encoded_bytes().is_ascii() {
            return self.is_match(candidate);
        }

        Ok(encoded::matches(
            encoded_program,
            self.inner.case_mode,
            self.inner.slash_mode,
            ctype,
            candidate.as_encoded_bytes(),
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{CompiledGlob, GlobCaseMode, GlobSlashMode, matches_pattern};
    use crate::ctype::resolve_ctype_profile_from;
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    /// The byte matcher may only stand in while the pattern is ASCII too: a
    /// non-ASCII character can fold onto an ASCII one.
    #[test]
    fn byte_matcher_does_not_stand_in_for_non_ascii_patterns() {
        let profile = resolve_ctype_profile_from([("LC_CTYPE", "en_US.UTF-8")]);
        assert!(!profile.is_byte_c(), "the test needs an encoded profile");

        // KELVIN SIGN lowercases to `k`, so this pattern matches `kilo` in a
        // UTF-8 locale, while byte folding cannot see the equivalence.
        let glob = CompiledGlob::compile_with_ctype(
            "-iname",
            OsStr::new("\u{212a}*"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
            &profile,
        )
        .unwrap();

        assert!(
            glob.is_match_with_ctype(OsStr::new("kilo"), &profile)
                .unwrap()
        );
        assert!(!glob.is_match(OsStr::new("kilo")).unwrap());
    }

    /// While both sides are ASCII the two matchers have to agree.
    #[test]
    fn ascii_candidates_get_the_same_answer_from_either_matcher() {
        let profile = resolve_ctype_profile_from([("LC_CTYPE", "en_US.UTF-8")]);

        for (pattern, candidate) in [
            ("*b", "aab"),
            ("a?c", "abc"),
            ("[!a]*", "bc"),
            ("a*", "a"),
            ("*", ""),
            ("*", "anything"),
            ("[a-c]*", "bzz"),
        ] {
            let glob = CompiledGlob::compile_with_ctype(
                "-iname",
                OsStr::new(pattern),
                GlobCaseMode::Insensitive,
                GlobSlashMode::Literal,
                &profile,
            )
            .unwrap();

            assert_eq!(
                glob.is_match_with_ctype(OsStr::new(candidate), &profile)
                    .unwrap(),
                glob.is_match(OsStr::new(candidate)).unwrap(),
                "{pattern:?} against {candidate:?}"
            );
        }
    }

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
    fn long_star_suffix_is_iterative() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("*z"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        let candidate = OsString::from_vec(vec![b'a'; 100_000]);

        assert!(!glob.is_match(candidate.as_os_str()).unwrap());
    }

    /// These used to cost exponential time, so terminating at all is half the
    /// assertion; the other half is the answer.
    #[test]
    fn many_stars_terminate_and_match_correctly() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("*a*a*a*a*a*a*a*a*b"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();

        assert!(
            !glob
                .is_match(OsString::from_vec(vec![b'a'; 20_000]).as_os_str())
                .unwrap()
        );

        let mut matching = vec![b'a'; 20_000];
        matching.push(b'b');
        assert!(
            glob.is_match(OsString::from_vec(matching).as_os_str())
                .unwrap()
        );
    }

    /// Expectations are `fnmatch(3)` with `FNM_PATHNAME`.
    #[test]
    fn pathname_mode_backtrack_stops_at_a_separator() {
        let deep = "a".repeat(20);
        for (pattern, candidate, expected) in [
            ("*b", "aa/b", false),
            ("*b", "aab", true),
            ("*.txt", "a/b.txt", false),
            ("*a*a*a*a*b", "aaaa/aaaa", false),
            ("*a*a*a*a*b", "aaaaab", true),
            ("*a*a*a*a*a*a*a*a*b", deep.as_str(), false),
        ] {
            assert_eq!(
                matches_pattern(OsStr::new(pattern), OsStr::new(candidate), false, true).unwrap(),
                expected,
                "{pattern:?} against {candidate:?}"
            );
        }
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
    fn encoded_locale_invalid_byte_patterns_fall_back_to_byte_matching() {
        let ctype = crate::ctype::resolve_ctype_profile_from([("LC_CTYPE", "C.UTF-8")]);
        let pattern = OsString::from_vec(vec![b'f', b'o', b'o', 0xff]);
        let candidate = OsString::from_vec(vec![b'f', b'o', b'o', 0xff]);
        let glob = CompiledGlob::compile_with_ctype(
            "-name",
            pattern.as_os_str(),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
            &ctype,
        )
        .unwrap();

        assert!(
            glob.is_match_with_ctype(candidate.as_os_str(), &ctype)
                .unwrap()
        );
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
