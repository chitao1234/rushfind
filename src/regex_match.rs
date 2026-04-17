use crate::diagnostics::Diagnostic;
use regex::bytes::{Regex, RegexBuilder};
use std::ffi::OsStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexDialect {
    Emacs,
    PosixExtended,
    Rust,
}

impl RegexDialect {
    pub fn parse(value: &OsStr) -> Result<Self, Diagnostic> {
        match value.to_str() {
            Some("emacs") => Ok(Self::Emacs),
            Some("posix-extended") => Ok(Self::PosixExtended),
            Some("rust") => Ok(Self::Rust),
            Some(other) => Err(Diagnostic::new(
                format!(
                    "unsupported `-regextype` value `{other}`; supported values: emacs, posix-extended, rust"
                ),
                1,
            )),
            None => Err(Diagnostic::new(
                "invalid UTF-8 `-regextype` value; supported values: emacs, posix-extended, rust",
                1,
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Emacs => "emacs",
            Self::PosixExtended => "posix-extended",
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
            RegexDialect::Emacs => translate_emacs_subset(flag, original_pattern)?,
            RegexDialect::PosixExtended => translate_posix_extended_subset(flag, original_pattern)?,
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

fn translate_posix_extended_subset(flag: &str, pattern: &str) -> Result<String, Diagnostic> {
    validate_common_subset(flag, "posix-extended", pattern)?;
    Ok(pattern.to_owned())
}

fn translate_emacs_subset(flag: &str, pattern: &str) -> Result<String, Diagnostic> {
    validate_common_subset(flag, "emacs", pattern)?;

    let mut translated = String::new();
    let mut chars = pattern.chars();
    let mut in_bracket_class = false;

    while let Some(ch) = chars.next() {
        match ch {
            '[' if !in_bracket_class => {
                in_bracket_class = true;
                translated.push(ch);
            }
            ']' if in_bracket_class => {
                in_bracket_class = false;
                translated.push(ch);
            }
            '\\' => {
                let escaped = chars.next().ok_or_else(|| {
                    Diagnostic::new(
                        format!("malformed emacs regex for `{flag}`: trailing `\\`"),
                        1,
                    )
                })?;
                translate_emacs_escape(flag, escaped, in_bracket_class, &mut translated)?;
            }
            '(' | ')' | '|' | '+' | '?' | '{' | '}' if !in_bracket_class => {
                translated.push('\\');
                translated.push(ch);
            }
            _ => translated.push(ch),
        }
    }

    Ok(translated)
}

fn translate_emacs_escape(
    flag: &str,
    escaped: char,
    in_bracket_class: bool,
    translated: &mut String,
) -> Result<(), Diagnostic> {
    if escaped.is_ascii_digit() {
        return Err(unsupported_construct(
            flag,
            "emacs",
            "backreferences are out of scope",
        ));
    }

    if in_bracket_class {
        translated.push('\\');
        translated.push(escaped);
        return Ok(());
    }

    match escaped {
        '(' | ')' | '|' | '+' | '?' | '{' | '}' => translated.push(escaped),
        '\\' | '.' | '^' | '$' | '*' | '[' | ']' => {
            translated.push('\\');
            translated.push(escaped);
        }
        other => {
            return Err(unsupported_construct(
                flag,
                "emacs",
                format!("unsupported escape `\\{other}`"),
            ));
        }
    }

    Ok(())
}

fn validate_common_subset(flag: &str, dialect: &str, pattern: &str) -> Result<(), Diagnostic> {
    if pattern.contains("[:") && pattern.contains(":]") {
        return Err(unsupported_construct(
            flag,
            dialect,
            "POSIX named character classes are out of scope",
        ));
    }
    if pattern.contains("[.") && pattern.contains(".]") {
        return Err(unsupported_construct(
            flag,
            dialect,
            "POSIX collating symbols are out of scope",
        ));
    }
    if pattern.contains("[=") && pattern.contains("=]") {
        return Err(unsupported_construct(
            flag,
            dialect,
            "POSIX equivalence classes are out of scope",
        ));
    }
    if contains_backreference(pattern) {
        return Err(unsupported_construct(
            flag,
            dialect,
            "backreferences are out of scope",
        ));
    }

    Ok(())
}

fn contains_backreference(pattern: &str) -> bool {
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            if ch.is_ascii_digit() {
                return true;
            }
            escaped = false;
            continue;
        }
        escaped = ch == '\\';
    }
    false
}

fn unsupported_construct(flag: &str, dialect: &str, reason: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::new(
        format!("unsupported construct in {dialect} regex for `{flag}`: {reason}"),
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
    fn emacs_subset_rejects_unescaped_grouping() {
        let error =
            RegexMatcher::compile("-regex", RegexDialect::Emacs, OsStr::new("\\(src"), false)
                .unwrap_err();

        assert!(error.message.contains("failed to compile emacs regex"));
    }
}
