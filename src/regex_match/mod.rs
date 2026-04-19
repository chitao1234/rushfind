mod backend;

use crate::diagnostics::Diagnostic;
use backend::{
    CompiledRegex, RegexBackendKind, compile_pcre2_anchored, compile_rust_anchored,
};
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
        let translated_pattern = match dialect {
            RegexDialect::Emacs | RegexDialect::PosixExtended | RegexDialect::PosixBasic => {
                translate_gnu_facing_subset(flag, dialect, &original_pattern)?
            }
            RegexDialect::Rust => translate_rust_bytes(&original_pattern),
            RegexDialect::Pcre2 => std::str::from_utf8(&original_pattern)
                .map(str::to_owned)
                .map_err(|_| {
                    Diagnostic::new(
                        format!(
                            "failed to compile pcre2 regex for `{flag}`: raw pcre2 patterns must be valid UTF-8; use PCRE2 byte escapes like `\\\\xFF` for arbitrary bytes"
                        ),
                        1,
                    )
                })?,
        };
        let anchored_pattern = format!(r"\A(?:{})\z", translated_pattern);
        let (backend, compiled) = match dialect {
            RegexDialect::Emacs | RegexDialect::PosixExtended | RegexDialect::PosixBasic | RegexDialect::Rust => (
                RegexBackendKind::Rust,
                compile_rust_anchored(
                    flag,
                    dialect.label(),
                    &anchored_pattern,
                    case_insensitive,
                )?,
            ),
            RegexDialect::Pcre2 => (
                RegexBackendKind::Pcre2,
                compile_pcre2_anchored(flag, &anchored_pattern, case_insensitive)?,
            ),
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

fn translate_gnu_facing_subset(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
) -> Result<String, Diagnostic> {
    let rules = GnuDialectRules::for_dialect(dialect);
    GnuRegexScanner::new(flag, dialect, rules, pattern).translate()
}

fn translate_rust_bytes(pattern: &[u8]) -> String {
    let mut translated = String::new();
    for &byte in pattern {
        push_rust_pattern_byte(&mut translated, byte);
    }
    translated
}

#[derive(Debug, Clone, Copy)]
struct GnuDialectRules {
    emacs_syntax: bool,
    posix_basic_syntax: bool,
    posix_extended_syntax: bool,
}

impl GnuDialectRules {
    fn for_dialect(dialect: RegexDialect) -> Self {
        match dialect {
            RegexDialect::Emacs => Self {
                emacs_syntax: true,
                posix_basic_syntax: false,
                posix_extended_syntax: false,
            },
            RegexDialect::PosixBasic => Self {
                emacs_syntax: false,
                posix_basic_syntax: true,
                posix_extended_syntax: false,
            },
            RegexDialect::PosixExtended => Self {
                emacs_syntax: false,
                posix_basic_syntax: false,
                posix_extended_syntax: true,
            },
            RegexDialect::Rust | RegexDialect::Pcre2 => {
                unreachable!("direct backend regexes are not translated as GNU subsets")
            }
        }
    }
}

struct GnuRegexScanner<'a> {
    flag: &'a str,
    dialect: RegexDialect,
    rules: GnuDialectRules,
    bytes: &'a [u8],
    index: usize,
    translated: String,
    group_depth: usize,
    can_repeat_atom: bool,
    branch_start: bool,
}

impl<'a> GnuRegexScanner<'a> {
    fn new(
        flag: &'a str,
        dialect: RegexDialect,
        rules: GnuDialectRules,
        pattern: &'a [u8],
    ) -> Self {
        Self {
            flag,
            dialect,
            rules,
            bytes: pattern,
            index: 0,
            translated: String::new(),
            group_depth: 0,
            can_repeat_atom: false,
            branch_start: true,
        }
    }

    fn translate(mut self) -> Result<String, Diagnostic> {
        while let Some(byte) = self.next() {
            match byte {
                b'[' => self.translate_bracket_expression()?,
                b'\\' => self.translate_escape()?,
                b'^' if self.rules.emacs_syntax => self.translate_emacs_caret(),
                b'$' if self.rules.emacs_syntax => self.translate_emacs_dollar(),
                b'*' if self.rules.emacs_syntax => self.translate_contextual_postfix(b'*'),
                b'+' | b'?' if self.rules.emacs_syntax => self.translate_contextual_postfix(byte),
                b'(' if self.rules.posix_extended_syntax => {
                    if self.peek() == Some(b'?') {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "non-capturing groups are out of scope",
                        ));
                    }
                    self.open_group();
                }
                b')' if self.rules.posix_extended_syntax => self.close_group_operator()?,
                b'|' if self.rules.posix_extended_syntax => self.push_alternation_operator(),
                b'*' if self.rules.posix_basic_syntax => self.translated.push('*'),
                b'*' | b'+' | b'?' if self.rules.posix_extended_syntax => {
                    self.translated.push(char::from(byte));
                }
                b'{' | b'}' if self.rules.posix_extended_syntax => {
                    self.translated.push(char::from(byte));
                }
                b'(' | b')' | b'|' | b'+' | b'?' | b'{' | b'}' if self.rules.posix_basic_syntax => {
                    self.push_literal_atom(byte);
                }
                b'(' | b')' | b'|' | b'{' | b'}' if self.rules.emacs_syntax => {
                    self.push_literal_atom(byte);
                }
                b'.' => {
                    self.translated.push('.');
                    self.mark_atom();
                }
                _ => self.push_literal_atom(byte),
            }
        }

        if self.group_depth != 0 {
            return Err(malformed_regex(self.flag, self.dialect, "unclosed group"));
        }

        Ok(self.translated)
    }

    fn translate_escape(&mut self) -> Result<(), Diagnostic> {
        let escaped = self
            .next()
            .ok_or_else(|| malformed_regex(self.flag, self.dialect, "trailing `\\`"))?;

        if escaped.is_ascii_digit() {
            return Err(unsupported_construct(
                self.flag,
                self.dialect,
                "backreferences are out of scope",
            ));
        }

        if self.rules.posix_basic_syntax {
            match escaped {
                b'(' => self.open_group(),
                b')' => self.close_group_operator()?,
                b'|' => self.push_alternation_operator(),
                b'+' | b'?' => self.translated.push(char::from(escaped)),
                b'{' => self.translate_bre_bound()?,
                b'\\' | b'.' | b'^' | b'$' | b'*' | b'[' | b']' | b'}' => {
                    self.push_literal_atom(escaped)
                }
                other => {
                    return Err(unsupported_construct(
                        self.flag,
                        self.dialect,
                        format!("unsupported escape `{}`", escaped_display(other)),
                    ));
                }
            }
        } else if self.rules.emacs_syntax {
            match escaped {
                b'(' => self.open_group(),
                b')' => self.close_group_operator()?,
                b'|' => self.push_alternation_operator(),
                b'{' => {
                    if self.can_repeat_atom {
                        self.translate_bre_bound()?;
                    } else {
                        self.push_literal_atom(b'{');
                    }
                }
                b'\\' | b'.' | b'^' | b'$' | b'*' | b'+' | b'?' | b'[' | b']' | b'}' => {
                    self.push_literal_atom(escaped)
                }
                other => self.push_literal_atom(other),
            }
        } else {
            match escaped {
                b'\\' | b'.' | b'^' | b'$' | b'*' | b'+' | b'?' | b'(' | b')' | b'|' | b'{'
                | b'}' | b'[' | b']' => self.push_literal_atom(escaped),
                other => {
                    return Err(unsupported_construct(
                        self.flag,
                        self.dialect,
                        format!("unsupported escape `{}`", escaped_display(other)),
                    ));
                }
            }
        }

        Ok(())
    }

    fn translate_bre_bound(&mut self) -> Result<(), Diagnostic> {
        let mut contents = String::new();

        loop {
            let byte = self.next().ok_or_else(|| {
                malformed_regex(self.flag, self.dialect, "unterminated bounded repetition")
            })?;

            if byte == b'\\' {
                let escaped = self.next().ok_or_else(|| {
                    malformed_regex(self.flag, self.dialect, "unterminated bounded repetition")
                })?;
                if escaped == b'}' {
                    break;
                }
                return Err(malformed_regex(
                    self.flag,
                    self.dialect,
                    "malformed bounded repetition",
                ));
            }

            if !byte.is_ascii() {
                return Err(malformed_regex(
                    self.flag,
                    self.dialect,
                    "malformed bounded repetition",
                ));
            }
            contents.push(char::from(byte));
        }

        let normalized = normalize_repetition_bound(&contents).ok_or_else(|| {
            malformed_regex(self.flag, self.dialect, "malformed bounded repetition")
        })?;

        self.translated.push('{');
        self.translated.push_str(&normalized);
        self.translated.push('}');
        self.can_repeat_atom = false;
        self.branch_start = false;
        Ok(())
    }

    fn translate_bracket_expression(&mut self) -> Result<(), Diagnostic> {
        self.translated.push('[');

        if self.peek() == Some(b'^') {
            self.index += 1;
            self.translated.push('^');
        }

        if self.peek() == Some(b']') {
            self.index += 1;
            self.translated.push(']');
        }

        while let Some(byte) = self.next() {
            match byte {
                b']' => {
                    self.translated.push(']');
                    self.mark_atom();
                    return Ok(());
                }
                b'[' => match self.peek() {
                    Some(b':') => {
                        self.index += 1;
                        let name = self.take_posix_class_name()?;
                        let fragment = posix_named_class_fragment(self.flag, self.dialect, &name)?;
                        self.translated.push_str(fragment);
                    }
                    Some(b'.') => {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "POSIX collating symbols are out of scope",
                        ));
                    }
                    Some(b'=') => {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "POSIX equivalence classes are out of scope",
                        ));
                    }
                    _ => self.translated.push('['),
                },
                b'\\' => {
                    if self.rules.emacs_syntax {
                        self.translated.push('\\');
                        self.translated.push('\\');
                    } else {
                        let escaped = self.next().ok_or_else(|| {
                            malformed_regex(
                                self.flag,
                                self.dialect,
                                "unterminated bracket expression",
                            )
                        })?;
                        push_bracket_escaped_byte(&mut self.translated, escaped);
                    }
                }
                _ => push_bracket_literal_byte(&mut self.translated, byte),
            }
        }

        Err(malformed_regex(
            self.flag,
            self.dialect,
            "unterminated bracket expression",
        ))
    }

    fn take_posix_class_name(&mut self) -> Result<String, Diagnostic> {
        let mut name = String::new();

        loop {
            let byte = self.next().ok_or_else(|| {
                malformed_regex(
                    self.flag,
                    self.dialect,
                    "unterminated POSIX character class",
                )
            })?;

            if byte == b':' && self.peek() == Some(b']') {
                self.index += 1;
                return Ok(name);
            }

            if !byte.is_ascii() {
                return Err(unsupported_construct(
                    self.flag,
                    self.dialect,
                    "unsupported POSIX character class",
                ));
            }

            name.push(char::from(byte));
        }
    }

    fn close_group(&mut self) -> Result<(), Diagnostic> {
        if self.group_depth == 0 {
            return Err(malformed_regex(self.flag, self.dialect, "unmatched `)`"));
        }
        self.group_depth -= 1;
        Ok(())
    }

    fn open_group(&mut self) {
        self.group_depth += 1;
        self.translated.push('(');
        self.can_repeat_atom = false;
        self.branch_start = true;
    }

    fn close_group_operator(&mut self) -> Result<(), Diagnostic> {
        self.close_group()?;
        self.translated.push(')');
        self.mark_atom();
        Ok(())
    }

    fn push_alternation_operator(&mut self) {
        self.translated.push('|');
        self.can_repeat_atom = false;
        self.branch_start = true;
    }

    fn translate_emacs_caret(&mut self) {
        if self.branch_start {
            self.translated.push('^');
            self.can_repeat_atom = false;
            self.branch_start = false;
        } else {
            self.push_literal_atom(b'^');
        }
    }

    fn translate_emacs_dollar(&mut self) {
        if self.peek().is_none()
            || (self.peek() == Some(b'\\') && matches!(self.peek_n(1), Some(b')') | Some(b'|')))
        {
            self.translated.push('$');
            self.can_repeat_atom = false;
            self.branch_start = false;
        } else {
            self.push_literal_atom(b'$');
        }
    }

    fn translate_contextual_postfix(&mut self, byte: u8) {
        if self.can_repeat_atom {
            self.translated.push(char::from(byte));
            self.can_repeat_atom = false;
            self.branch_start = false;
        } else {
            self.push_literal_atom(byte);
        }
    }

    fn push_literal_atom(&mut self, byte: u8) {
        push_literal_regex_byte(&mut self.translated, byte);
        self.mark_atom();
    }

    fn mark_atom(&mut self) {
        self.can_repeat_atom = true;
        self.branch_start = false;
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.bytes.get(self.index).copied()?;
        self.index += 1;
        Some(byte)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn peek_n(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.index + offset).copied()
    }
}

fn push_rust_pattern_byte(out: &mut String, byte: u8) {
    match byte {
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_literal_regex_byte(out: &mut String, byte: u8) {
    match byte {
        b'.' | b'^' | b'$' | b'|' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'*' | b'+'
        | b'?' | b'\\' => {
            out.push('\\');
            out.push(char::from(byte));
        }
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_bracket_literal_byte(out: &mut String, byte: u8) {
    match byte {
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_bracket_escaped_byte(out: &mut String, byte: u8) {
    match byte {
        b'\\' | b']' | b'^' | b'-' => {
            out.push('\\');
            out.push(char::from(byte));
        }
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_hex_byte(out: &mut String, byte: u8) {
    write!(out, r"\x{:02X}", byte).unwrap();
}

fn escaped_display(byte: u8) -> String {
    match byte {
        0x20..=0x7e => format!(r"\{}", char::from(byte)),
        _ => format!(r"\x{:02X}", byte),
    }
}

fn normalize_repetition_bound(contents: &str) -> Option<String> {
    if let Some((left, right)) = contents.split_once(',') {
        let left_valid = left.is_empty() || left.chars().all(|ch| ch.is_ascii_digit());
        let right_valid = right.is_empty() || right.chars().all(|ch| ch.is_ascii_digit());

        if !left_valid || !right_valid {
            return None;
        }

        let lower = if left.is_empty() { "0" } else { left };
        Some(format!("{lower},{right}"))
    } else {
        (!contents.is_empty() && contents.chars().all(|ch| ch.is_ascii_digit()))
            .then(|| contents.to_owned())
    }
}

fn posix_named_class_fragment(
    flag: &str,
    dialect: RegexDialect,
    name: &str,
) -> Result<&'static str, Diagnostic> {
    match name {
        "alnum" => Ok("A-Za-z0-9"),
        "alpha" => Ok("A-Za-z"),
        "blank" => Ok(r" \t"),
        "cntrl" => Ok(r"\x00-\x1F\x7F"),
        "digit" => Ok("0-9"),
        "graph" => Ok("!-~"),
        "lower" => Ok("a-z"),
        "print" => Ok(r"\x20-\x7E"),
        "punct" => Ok(r"!-/:-@\x5B-\x60{-~"),
        "space" => Ok(r" \t\r\n\f\x0B"),
        "upper" => Ok("A-Z"),
        "xdigit" => Ok("A-Fa-f0-9"),
        other => Err(unsupported_construct(
            flag,
            dialect,
            format!("unsupported POSIX character class `[:{other}:]`"),
        )),
    }
}

fn unsupported_construct(
    flag: &str,
    dialect: RegexDialect,
    reason: impl std::fmt::Display,
) -> Diagnostic {
    Diagnostic::new(
        format!(
            "unsupported construct in {} regex for `{flag}`: {reason}",
            dialect.label()
        ),
        1,
    )
}

fn malformed_regex(
    flag: &str,
    dialect: RegexDialect,
    reason: impl std::fmt::Display,
) -> Diagnostic {
    Diagnostic::new(
        format!("malformed {} regex for `{flag}`: {reason}", dialect.label()),
        1,
    )
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
    fn gnu_facing_dialects_reject_backreferences() {
        for dialect in [
            RegexDialect::Emacs,
            RegexDialect::PosixExtended,
            RegexDialect::PosixBasic,
        ] {
            let error =
                RegexMatcher::compile("-regex", dialect, OsStr::new("\\1"), false).unwrap_err();

            assert!(error.message.contains(dialect.label()));
            assert!(error.message.contains("backreferences are out of scope"));
        }
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
