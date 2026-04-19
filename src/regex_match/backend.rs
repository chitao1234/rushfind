use crate::diagnostics::Diagnostic;
use pcre2::bytes::{Regex as Pcre2Regex, RegexBuilder as Pcre2RegexBuilder};
use regex::bytes::{Regex as RustRegex, RegexBuilder as RustRegexBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexBackendKind {
    Rust,
    Pcre2,
}

#[derive(Debug, Clone)]
pub enum CompiledRegex {
    Rust(RustRegex),
    Pcre2(Pcre2Regex),
}

pub fn compile_rust_anchored(
    flag: &str,
    dialect_label: &str,
    anchored_pattern: &str,
    case_insensitive: bool,
) -> Result<CompiledRegex, Diagnostic> {
    let compiled = RustRegexBuilder::new(anchored_pattern)
        .case_insensitive(case_insensitive)
        .unicode(false)
        .build()
        .map_err(|error| {
            Diagnostic::new(
                format!("failed to compile {dialect_label} regex for `{flag}`: {error}"),
                1,
            )
        })?;
    Ok(CompiledRegex::Rust(compiled))
}

pub fn compile_pcre2_anchored(
    flag: &str,
    anchored_pattern: &str,
    case_insensitive: bool,
) -> Result<CompiledRegex, Diagnostic> {
    let mut builder = Pcre2RegexBuilder::new();
    builder.caseless(case_insensitive);
    let compiled = builder.build(anchored_pattern).map_err(|error| {
        Diagnostic::new(
            format!("failed to compile pcre2 regex for `{flag}`: {error}"),
            1,
        )
    })?;
    Ok(CompiledRegex::Pcre2(compiled))
}

impl CompiledRegex {
    pub fn is_match(&self, candidate: &[u8]) -> Result<bool, Diagnostic> {
        match self {
            Self::Rust(regex) => Ok(regex.is_match(candidate)),
            Self::Pcre2(regex) => regex.is_match(candidate).map_err(|error| {
                Diagnostic::new(format!("failed to execute pcre2 regex: {error}"), 1)
            }),
        }
    }
}
