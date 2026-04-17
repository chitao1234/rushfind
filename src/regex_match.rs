use crate::diagnostics::Diagnostic;
use regex::bytes::{Regex, RegexBuilder};
use std::ffi::OsStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexDialect {
    Emacs,
    PosixExtended,
    PosixBasic,
    Rust,
}

impl RegexDialect {
    pub fn parse(value: &OsStr) -> Result<Self, Diagnostic> {
        match value.to_str() {
            Some("emacs") => Ok(Self::Emacs),
            Some("posix-extended") => Ok(Self::PosixExtended),
            Some("posix-basic") => Ok(Self::PosixBasic),
            Some("rust") => Ok(Self::Rust),
            Some(other) => Err(Diagnostic::new(
                format!(
                    "unsupported `-regextype` value `{other}`; supported values: emacs, posix-extended, posix-basic, rust"
                ),
                1,
            )),
            None => Err(Diagnostic::new(
                "invalid UTF-8 `-regextype` value; supported values: emacs, posix-extended, posix-basic, rust",
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
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegexMatcher {
    dialect: RegexDialect,
    original_pattern: String,
    translated_pattern: String,
    case_insensitive: bool,
    compiled: Regex,
}

impl PartialEq for RegexMatcher {
    fn eq(&self, other: &Self) -> bool {
        self.dialect == other.dialect
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
        let original_pattern = pattern.to_str().ok_or_else(|| {
            Diagnostic::new(format!("invalid UTF-8 regex pattern for `{flag}`"), 1)
        })?;
        let translated_pattern = match dialect {
            RegexDialect::Emacs | RegexDialect::PosixExtended | RegexDialect::PosixBasic => {
                translate_gnu_facing_subset(flag, dialect, original_pattern)?
            }
            RegexDialect::Rust => original_pattern.to_owned(),
        };
        let compiled = RegexBuilder::new(&format!(r"\A(?:{})\z", translated_pattern))
            .case_insensitive(case_insensitive)
            .unicode(false)
            .build()
            .map_err(|error| {
                Diagnostic::new(
                    format!(
                        "failed to compile {} regex for `{flag}`: {error}",
                        dialect.label()
                    ),
                    1,
                )
            })?;

        Ok(Self {
            dialect,
            original_pattern: original_pattern.to_owned(),
            translated_pattern,
            case_insensitive,
            compiled,
        })
    }

    pub fn dialect(&self) -> RegexDialect {
        self.dialect
    }

    pub fn is_match(&self, candidate: &OsStr) -> bool {
        self.compiled.is_match(candidate.as_encoded_bytes())
    }
}

fn translate_gnu_facing_subset(
    flag: &str,
    dialect: RegexDialect,
    pattern: &str,
) -> Result<String, Diagnostic> {
    let rules = GnuDialectRules::for_dialect(dialect);
    GnuRegexScanner::new(flag, dialect, rules, pattern).translate()
}

#[derive(Debug, Clone, Copy)]
struct GnuDialectRules {
    bre_style_operators: bool,
}

impl GnuDialectRules {
    fn for_dialect(dialect: RegexDialect) -> Self {
        match dialect {
            RegexDialect::Emacs | RegexDialect::PosixBasic => Self {
                bre_style_operators: true,
            },
            RegexDialect::PosixExtended => Self {
                bre_style_operators: false,
            },
            RegexDialect::Rust => unreachable!("rust regexes are not translated as GNU subsets"),
        }
    }
}

struct GnuRegexScanner<'a> {
    flag: &'a str,
    dialect: RegexDialect,
    rules: GnuDialectRules,
    chars: Vec<char>,
    index: usize,
    translated: String,
    group_depth: usize,
}

impl<'a> GnuRegexScanner<'a> {
    fn new(flag: &'a str, dialect: RegexDialect, rules: GnuDialectRules, pattern: &str) -> Self {
        Self {
            flag,
            dialect,
            rules,
            chars: pattern.chars().collect(),
            index: 0,
            translated: String::new(),
            group_depth: 0,
        }
    }

    fn translate(mut self) -> Result<String, Diagnostic> {
        while let Some(ch) = self.next() {
            match ch {
                '[' => self.translate_bracket_expression()?,
                '\\' => self.translate_escape()?,
                '(' if !self.rules.bre_style_operators => {
                    if self.peek() == Some('?') {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "non-capturing groups are out of scope",
                        ));
                    }
                    self.group_depth += 1;
                    self.translated.push(ch);
                }
                ')' if !self.rules.bre_style_operators => {
                    self.close_group()?;
                    self.translated.push(ch);
                }
                '(' | ')' | '|' | '+' | '?' | '{' | '}' if self.rules.bre_style_operators => {
                    self.push_escaped_literal(ch);
                }
                _ => self.translated.push(ch),
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

        if self.rules.bre_style_operators {
            match escaped {
                '(' => {
                    self.group_depth += 1;
                    self.translated.push('(');
                }
                ')' => {
                    self.close_group()?;
                    self.translated.push(')');
                }
                '|' | '+' | '?' => self.translated.push(escaped),
                '{' => self.translate_bre_bound()?,
                '\\' | '.' | '^' | '$' | '*' | '[' | ']' | '}' => {
                    self.push_escaped_literal(escaped);
                }
                other => {
                    return Err(unsupported_construct(
                        self.flag,
                        self.dialect,
                        format!("unsupported escape `\\{other}`"),
                    ));
                }
            }
        } else {
            match escaped {
                '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '|' | '{' | '}' | '['
                | ']' => self.push_escaped_literal(escaped),
                other => {
                    return Err(unsupported_construct(
                        self.flag,
                        self.dialect,
                        format!("unsupported escape `\\{other}`"),
                    ));
                }
            }
        }

        Ok(())
    }

    fn translate_bre_bound(&mut self) -> Result<(), Diagnostic> {
        let mut contents = String::new();

        loop {
            let ch = self.next().ok_or_else(|| {
                malformed_regex(self.flag, self.dialect, "unterminated bounded repetition")
            })?;

            if ch == '\\' {
                let escaped = self.next().ok_or_else(|| {
                    malformed_regex(self.flag, self.dialect, "unterminated bounded repetition")
                })?;
                if escaped == '}' {
                    break;
                }
                return Err(malformed_regex(
                    self.flag,
                    self.dialect,
                    "malformed bounded repetition",
                ));
            }

            contents.push(ch);
        }

        if !is_valid_repetition_bound(&contents) {
            return Err(malformed_regex(
                self.flag,
                self.dialect,
                "malformed bounded repetition",
            ));
        }

        self.translated.push('{');
        self.translated.push_str(&contents);
        self.translated.push('}');
        Ok(())
    }

    fn translate_bracket_expression(&mut self) -> Result<(), Diagnostic> {
        self.translated.push('[');

        if self.peek() == Some('^') {
            self.index += 1;
            self.translated.push('^');
        }

        if self.peek() == Some(']') {
            self.index += 1;
            self.translated.push(']');
        }

        while let Some(ch) = self.next() {
            match ch {
                ']' => {
                    self.translated.push(']');
                    return Ok(());
                }
                '[' => match self.peek() {
                    Some(':') => {
                        self.index += 1;
                        let name = self.take_posix_class_name()?;
                        let fragment = posix_named_class_fragment(self.flag, self.dialect, &name)?;
                        self.translated.push_str(fragment);
                    }
                    Some('.') => {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "POSIX collating symbols are out of scope",
                        ));
                    }
                    Some('=') => {
                        return Err(unsupported_construct(
                            self.flag,
                            self.dialect,
                            "POSIX equivalence classes are out of scope",
                        ));
                    }
                    _ => self.translated.push('['),
                },
                '\\' => {
                    let escaped = self.next().ok_or_else(|| {
                        malformed_regex(self.flag, self.dialect, "unterminated bracket expression")
                    })?;
                    self.translated.push('\\');
                    self.translated.push(escaped);
                }
                _ => self.translated.push(ch),
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
            let ch = self.next().ok_or_else(|| {
                malformed_regex(
                    self.flag,
                    self.dialect,
                    "unterminated POSIX character class",
                )
            })?;

            if ch == ':' && self.peek() == Some(']') {
                self.index += 1;
                return Ok(name);
            }

            name.push(ch);
        }
    }

    fn close_group(&mut self) -> Result<(), Diagnostic> {
        if self.group_depth == 0 {
            return Err(malformed_regex(self.flag, self.dialect, "unmatched `)`"));
        }
        self.group_depth -= 1;
        Ok(())
    }

    fn push_escaped_literal(&mut self, ch: char) {
        self.translated.push('\\');
        self.translated.push(ch);
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.get(self.index).copied()?;
        self.index += 1;
        Some(ch)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }
}

fn is_valid_repetition_bound(contents: &str) -> bool {
    if let Some((left, right)) = contents.split_once(',') {
        !left.is_empty()
            && left.chars().all(|ch| ch.is_ascii_digit())
            && (right.is_empty() || right.chars().all(|ch| ch.is_ascii_digit()))
    } else {
        !contents.is_empty() && contents.chars().all(|ch| ch.is_ascii_digit())
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
    use std::ffi::OsStr;

    #[test]
    fn rust_mode_wraps_patterns_to_match_the_full_path() {
        let matcher =
            RegexMatcher::compile("-regex", RegexDialect::Rust, OsStr::new(".*\\.rs"), false)
                .unwrap();

        assert!(matcher.is_match(OsStr::new("./src/lib.rs")));
        assert!(!matcher.is_match(OsStr::new("lib")));
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

            assert!(matcher.is_match(OsStr::new("./A7")));
            assert!(!matcher.is_match(OsStr::new("./é7")));
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

        assert!(matcher.is_match(OsStr::new("./name.txt")));
        assert!(!matcher.is_match(OsStr::new("./ .txt")));
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
