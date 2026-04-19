mod backend;
mod gnu;
mod ir;

use crate::diagnostics::Diagnostic;
use backend::{
    CompiledRegex, RegexBackendKind, compile_pcre2_anchored, compile_rust_anchored,
};
use gnu::compile_gnu_regex;
use std::ffi::OsStr;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexDialect {
    Emacs,
    PosixExtended,
    PosixBasic,
    Rust,
    Pcre2,
}

impl RegexDialect {
    pub fn parse(value: &OsStr) -> Result<Self, Diagnostic> {
        match value.to_str() {
            Some("emacs") => Ok(Self::Emacs),
            Some("posix-extended") => Ok(Self::PosixExtended),
            Some("posix-basic") => Ok(Self::PosixBasic),
            Some("rust") => Ok(Self::Rust),
            Some("pcre2") => Ok(Self::Pcre2),
            Some(other) => Err(Diagnostic::new(
                format!(
                    "unsupported `-regextype` value `{other}`; supported values: emacs, posix-extended, posix-basic, rust, pcre2"
                ),
                1,
            )),
            None => Err(Diagnostic::new(
                "invalid UTF-8 `-regextype` value; supported values: emacs, posix-extended, posix-basic, rust, pcre2",
                1,
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Emacs => "emacs",
            Self::PosixExtended => "posix-extended",
            Self::PosixBasic => "posix-basic",
            Self::Rust => "rust",
            Self::Pcre2 => "pcre2",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegexMatcher {
    dialect: RegexDialect,
    backend: RegexBackendKind,
    original_pattern: Vec<u8>,
    translated_pattern: String,
    case_insensitive: bool,
    compiled: CompiledRegex,
}

impl PartialEq for RegexMatcher {
    fn eq(&self, other: &Self) -> bool {
        self.dialect == other.dialect
            && self.backend == other.backend
            && self.original_pattern == other.original_pattern
            && self.translated_pattern == other.translated_pattern
            && self.case_insensitive == other.case_insensitive
    }
}

impl Eq for RegexMatcher {}

impl RegexMatcher {
    pub fn compile(
        flag: &str,
        dialect: RegexDialect,
        pattern: &OsStr,
        case_insensitive: bool,
    ) -> Result<Self, Diagnostic> {
        let original_pattern = pattern.as_encoded_bytes().to_vec();

        let (backend, translated_pattern, compiled) = match dialect {
            RegexDialect::Emacs | RegexDialect::PosixExtended | RegexDialect::PosixBasic => {
                let compiled = compile_gnu_regex(flag, dialect, &original_pattern, case_insensitive)?;
                (compiled.backend, compiled.translated_pattern, compiled.compiled)
            }
            RegexDialect::Rust => {
                let translated_pattern = translate_rust_bytes(&original_pattern);
                let anchored_pattern = format!(r"\A(?:{})\z", translated_pattern);
                let compiled =
                    compile_rust_anchored(flag, dialect.label(), &anchored_pattern, case_insensitive)?;
                (RegexBackendKind::Rust, translated_pattern, compiled)
            }
            RegexDialect::Pcre2 => {
                let translated_pattern = std::str::from_utf8(&original_pattern)
                    .map(str::to_owned)
                    .map_err(|_| {
                        Diagnostic::new(
                            format!(
                                "failed to compile pcre2 regex for `{flag}`: raw pcre2 patterns must be valid UTF-8; use PCRE2 byte escapes like `\\\\xFF` for arbitrary bytes"
                            ),
                            1,
                        )
                    })?;
                let anchored_pattern = format!(r"\A(?:{})\z", translated_pattern);
                let compiled = compile_pcre2_anchored(flag, &anchored_pattern, case_insensitive)?;
                (RegexBackendKind::Pcre2, translated_pattern, compiled)
            }
        };

        Ok(Self {
            dialect,
            backend,
            original_pattern,
            translated_pattern,
            case_insensitive,
            compiled,
        })
    }

    pub fn dialect(&self) -> RegexDialect {
        self.dialect
    }

    #[cfg(test)]
    pub(crate) fn backend_kind(&self) -> RegexBackendKind {
        self.backend
    }

    pub fn is_match(&self, candidate: &OsStr) -> Result<bool, Diagnostic> {
        self.compiled.is_match(candidate.as_encoded_bytes())
    }
}

fn translate_rust_bytes(pattern: &[u8]) -> String {
    let mut translated = String::new();
    for &byte in pattern {
        match byte {
            0x20..=0x7e => translated.push(char::from(byte)),
            other => push_hex_byte(&mut translated, other),
        }
    }
    translated
}

fn push_hex_byte(out: &mut String, byte: u8) {
    write!(out, r"\x{:02X}", byte).unwrap();
}

#[cfg(test)]
mod tests {
    use super::{RegexDialect, RegexMatcher};
    use crate::regex_match::backend::RegexBackendKind;
    use std::ffi::OsStr;
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn rust_mode_wraps_patterns_to_match_the_full_path() {
        let matcher =
            RegexMatcher::compile("-regex", RegexDialect::Rust, OsStr::new(".*\\.rs"), false)
                .unwrap();

        assert!(matcher.is_match(OsStr::new("./src/lib.rs")).unwrap());
        assert!(!matcher.is_match(OsStr::new("lib")).unwrap());
    }

    #[test]
    fn gnu_facing_dialects_support_c_locale_named_classes() {
        for dialect in [
            RegexDialect::Emacs,
            RegexDialect::PosixExtended,
            RegexDialect::PosixBasic,
        ] {
            let matcher = RegexMatcher::compile(
                "-regex",
                dialect,
                OsStr::new(".*[[:alpha:]][[:digit:]]"),
                false,
            )
            .unwrap();

            assert!(matcher.is_match(OsStr::new("./A7")).unwrap());
            assert!(!matcher.is_match(OsStr::new("./é7")).unwrap());
        }
    }

    #[test]
    fn named_classes_work_in_negated_bracket_expressions() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::PosixExtended,
            OsStr::new(".*[^[:space:]]\\.txt"),
            false,
        )
        .unwrap();

        assert!(matcher.is_match(OsStr::new("./name.txt")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./ .txt")).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_patterns_compile_in_rust_and_gnu_facing_modes() {
        let rust_pattern = OsString::from_vec(vec![b'.', b'*', b'/', b'f', b'o', b'o', 0xff]);
        let rust_candidate = OsString::from_vec(vec![b'.', b'/', b'f', b'o', b'o', 0xff]);
        let rust = RegexMatcher::compile(
            "-regex",
            RegexDialect::Rust,
            rust_pattern.as_os_str(),
            false,
        )
        .unwrap();
        assert!(rust.is_match(rust_candidate.as_os_str()).unwrap());

        let gnu_pattern = OsString::from_vec(vec![b'.', b'*', b'/', b'b', b'a', b'r', 0xfe]);
        let gnu_candidate = OsString::from_vec(vec![b'.', b'/', b'b', b'a', b'r', 0xfe]);
        let gnu = RegexMatcher::compile(
            "-regex",
            RegexDialect::Emacs,
            gnu_pattern.as_os_str(),
            false,
        )
        .unwrap();
        assert!(gnu.is_match(gnu_candidate.as_os_str()).unwrap());
    }

    #[test]
    fn emacs_uses_bare_plus_and_treats_backslashed_plus_as_literal() {
        let bare_plus =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(".*a+"), false)
                .unwrap();
        assert!(bare_plus.is_match(OsStr::new("./a")).unwrap());
        assert!(bare_plus.is_match(OsStr::new("./aa")).unwrap());
        assert!(!bare_plus.is_match(OsStr::new("./a+")).unwrap());

        let escaped_plus =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r".*a\+"), false)
                .unwrap();
        assert!(!escaped_plus.is_match(OsStr::new("./a")).unwrap());
        assert!(!escaped_plus.is_match(OsStr::new("./aa")).unwrap());
        assert!(escaped_plus.is_match(OsStr::new("./a+")).unwrap());
    }

    #[test]
    fn emacs_anchors_and_repetition_are_context_sensitive() {
        let caret_literal =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(".*a^b"), false)
                .unwrap();
        assert!(caret_literal.is_match(OsStr::new("./a^b")).unwrap());
        assert!(!caret_literal.is_match(OsStr::new("./ab")).unwrap());

        let leading_caret =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new("^\\./a"), false)
                .unwrap();
        assert!(leading_caret.is_match(OsStr::new("./a")).unwrap());
        assert!(!leading_caret.is_match(OsStr::new("x./a")).unwrap());
    }

    #[test]
    fn emacs_unknown_escapes_degrade_to_the_escaped_character() {
        let matcher =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r".*\a"), false)
                .unwrap();

        assert!(matcher.is_match(OsStr::new("./a")).unwrap());
        assert!(matcher.is_match(OsStr::new("./aa")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./b")).unwrap());
    }

    #[test]
    fn emacs_backslash_is_literal_inside_bracket_expressions() {
        let matcher =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r".*[\]]"), false)
                .unwrap();

        assert!(matcher.is_match(OsStr::new("./\\]")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./]")).unwrap());
    }

    #[test]
    fn emacs_intervals_allow_omitted_lower_bounds() {
        let up_to_two =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r"a\{,2\}"), false)
                .unwrap();
        assert!(up_to_two.is_match(OsStr::new("")).unwrap());
        assert!(up_to_two.is_match(OsStr::new("a")).unwrap());
        assert!(up_to_two.is_match(OsStr::new("aa")).unwrap());
        assert!(!up_to_two.is_match(OsStr::new("aaa")).unwrap());

        let unbounded =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r"a\{,\}"), false)
                .unwrap();
        assert!(unbounded.is_match(OsStr::new("")).unwrap());
        assert!(unbounded.is_match(OsStr::new("aaaa")).unwrap());
    }

    #[test]
    fn posix_basic_intervals_allow_omitted_lower_bounds() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::PosixBasic,
            OsStr::new(r"a\{,2\}"),
            false,
        )
        .unwrap();

        assert!(matcher.is_match(OsStr::new("")).unwrap());
        assert!(matcher.is_match(OsStr::new("aa")).unwrap());
        assert!(!matcher.is_match(OsStr::new("aaa")).unwrap());
    }

    #[test]
    fn direct_pcre2_mode_matches_raw_pcre2_syntax() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::Pcre2,
            OsStr::new(".*/(?:src|docs)/.+\\.(?:rs|txt)"),
            false,
        )
        .unwrap();

        assert_eq!(matcher.backend_kind(), RegexBackendKind::Pcre2);
        assert!(matcher.is_match(OsStr::new("./src/lib.rs")).unwrap());
        assert!(matcher.is_match(OsStr::new("./docs/Guide.txt")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./README.md")).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn direct_pcre2_mode_matches_non_utf8_candidates_via_hex_escape() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::Pcre2,
            OsStr::new(".*/foo\\xFF"),
            false,
        )
        .unwrap();
        let candidate = OsString::from_vec(vec![b'.', b'/', b'f', b'o', b'o', 0xff]);

        assert!(matcher.is_match(candidate.as_os_str()).unwrap());
    }

    #[test]
    fn gnu_foundation_backreferences_use_pcre2_backend() {
        for (dialect, pattern) in [
            (RegexDialect::PosixBasic, r".*/\(.\)\1"),
            (RegexDialect::PosixExtended, r".*/(.)\1"),
        ] {
            let matcher = RegexMatcher::compile("-regex", dialect, OsStr::new(pattern), false)
                .unwrap();

            assert_eq!(matcher.backend_kind(), RegexBackendKind::Pcre2);
            assert!(matcher.is_match(OsStr::new("./aa")).unwrap());
        }
    }

    #[test]
    fn emacs_followup_backreferences_use_pcre2_backend() {
        let matcher =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new(r".*/\(.\)\1"), false)
                .unwrap();

        assert_eq!(matcher.backend_kind(), RegexBackendKind::Pcre2);
        assert!(matcher.is_match(OsStr::new("./aa")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./ab")).unwrap());
    }

    #[test]
    fn emacs_followup_mixed_group_and_backreference_match() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::Emacs,
            OsStr::new(r".*/\(ab\|cd\)\1"),
            false,
        )
        .unwrap();

        assert_eq!(matcher.backend_kind(), RegexBackendKind::Pcre2);
        assert!(matcher.is_match(OsStr::new("./abab")).unwrap());
        assert!(matcher.is_match(OsStr::new("./cdcd")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./abcd")).unwrap());
    }

    #[test]
    fn gnu_review_followup_bre_and_ere_treat_backslash_as_literal_inside_bracket_expressions() {
        for dialect in [RegexDialect::PosixBasic, RegexDialect::PosixExtended] {
            let matcher =
                RegexMatcher::compile("-regex", dialect, OsStr::new(r".*/[a\b]"), false)
                    .unwrap();

            assert!(matcher.is_match(OsStr::new("./a")).unwrap());
            assert!(matcher.is_match(OsStr::new("./b")).unwrap());
            assert!(matcher.is_match(OsStr::new("./\\")).unwrap());
            assert!(!matcher.is_match(OsStr::new("./c")).unwrap());
        }
    }

    #[test]
    fn gnu_review_followup_bre_and_ere_support_byte_ranges() {
        for dialect in [RegexDialect::PosixBasic, RegexDialect::PosixExtended] {
            let matcher =
                RegexMatcher::compile("-regex", dialect, OsStr::new(r".*/[a-c]"), false)
                    .unwrap();

            assert!(matcher.is_match(OsStr::new("./a")).unwrap());
            assert!(matcher.is_match(OsStr::new("./b")).unwrap());
            assert!(matcher.is_match(OsStr::new("./c")).unwrap());
            assert!(!matcher.is_match(OsStr::new("./d")).unwrap());
        }
    }

    #[test]
    fn gnu_review_followup_bre_and_ere_reject_backward_ranges() {
        for dialect in [RegexDialect::PosixBasic, RegexDialect::PosixExtended] {
            let error =
                RegexMatcher::compile("-regex", dialect, OsStr::new(r".*/[z-a]"), false)
                    .unwrap_err();

            assert!(error.message.contains("invalid range"));
        }
    }

    #[test]
    fn gnu_foundation_boundary_escapes_use_pcre2_backend() {
        let matcher = RegexMatcher::compile(
            "-regex",
            RegexDialect::PosixExtended,
            OsStr::new(r".*/\<foo\>"),
            false,
        )
        .unwrap();

        assert_eq!(matcher.backend_kind(), RegexBackendKind::Pcre2);
        assert!(matcher.is_match(OsStr::new("./foo")).unwrap());
        assert!(!matcher.is_match(OsStr::new("./foobar")).unwrap());
    }

    #[test]
    fn gnu_facing_dialects_reject_collating_and_equivalence_classes() {
        for pattern in ["[[.ch.]]", "[[=a=]]"] {
            let error = RegexMatcher::compile(
                "-regex",
                RegexDialect::PosixExtended,
                OsStr::new(pattern),
                false,
            )
            .unwrap_err();

            assert!(error.message.contains("unsupported construct"));
        }
    }
}
