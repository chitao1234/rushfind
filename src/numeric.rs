use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericComparison {
    Exactly(u64),
    LessThan(u64),
    GreaterThan(u64),
}

impl NumericComparison {
    pub fn matches(self, actual: u64) -> bool {
        match self {
            Self::Exactly(expected) => actual == expected,
            Self::LessThan(expected) => actual < expected,
            Self::GreaterThan(expected) => actual > expected,
        }
    }
}

pub fn validate_numeric_argument(flag: &str, value: &OsStr) -> Result<(), Diagnostic> {
    parse_numeric_argument(flag, value).map(|_| ())
}

pub fn parse_numeric_argument(
    flag: &str,
    value: &OsStr,
) -> Result<NumericComparison, Diagnostic> {
    let bytes = value.as_encoded_bytes();
    let (kind, digits) = match bytes {
        [b'+', rest @ ..] => (NumericComparisonKind::GreaterThan, rest),
        [b'-', rest @ ..] => (NumericComparisonKind::LessThan, rest),
        _ => (NumericComparisonKind::Exactly, bytes),
    };

    if digits.is_empty() {
        return Err(invalid_numeric_argument(flag, value));
    }

    let rendered = String::from_utf8_lossy(digits);
    let parsed = rendered
        .parse::<u64>()
        .map_err(|_| invalid_numeric_argument(flag, value))?;

    Ok(match kind {
        NumericComparisonKind::Exactly => NumericComparison::Exactly(parsed),
        NumericComparisonKind::LessThan => NumericComparison::LessThan(parsed),
        NumericComparisonKind::GreaterThan => NumericComparison::GreaterThan(parsed),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericComparisonKind {
    Exactly,
    LessThan,
    GreaterThan,
}

fn invalid_numeric_argument(flag: &str, value: &OsStr) -> Diagnostic {
    Diagnostic::parse(format!(
        "invalid numeric argument for `{flag}`: `{}`",
        value.to_string_lossy()
    ))
}
