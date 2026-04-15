use crate::diagnostics::Diagnostic;
use crate::numeric::NumericComparison;
use std::ffi::OsStr;
use std::str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeUnit {
    Bytes,
    Words2,
    Blocks512,
    KiB,
    MiB,
    GiB,
}

impl SizeUnit {
    fn unit_bytes(self) -> u64 {
        match self {
            Self::Bytes => 1,
            Self::Words2 => 2,
            Self::Blocks512 => 512,
            Self::KiB => 1024,
            Self::MiB => 1024 * 1024,
            Self::GiB => 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeMatcher {
    pub comparison: NumericComparison,
    pub unit: SizeUnit,
}

impl SizeMatcher {
    pub fn matches(self, size_bytes: u64) -> bool {
        self.comparison
            .matches(rounded_up_units(size_bytes, self.unit.unit_bytes()))
    }
}

pub fn parse_size_argument(raw: &OsStr) -> Result<SizeMatcher, Diagnostic> {
    let bytes = raw.as_encoded_bytes();
    let (numeric_bytes, unit) = match bytes.last().copied() {
        Some(b'c') => (&bytes[..bytes.len() - 1], SizeUnit::Bytes),
        Some(b'w') => (&bytes[..bytes.len() - 1], SizeUnit::Words2),
        Some(b'b') => (&bytes[..bytes.len() - 1], SizeUnit::Blocks512),
        Some(b'k') => (&bytes[..bytes.len() - 1], SizeUnit::KiB),
        Some(b'M') => (&bytes[..bytes.len() - 1], SizeUnit::MiB),
        Some(b'G') => (&bytes[..bytes.len() - 1], SizeUnit::GiB),
        Some(b'0'..=b'9') | Some(b'+') | Some(b'-') => (bytes, SizeUnit::Blocks512),
        _ => return Err(invalid_size_argument(raw)),
    };

    Ok(SizeMatcher {
        comparison: parse_size_comparison(numeric_bytes, raw)?,
        unit,
    })
}

fn parse_size_comparison(bytes: &[u8], raw: &OsStr) -> Result<NumericComparison, Diagnostic> {
    let (kind, digits) = match bytes {
        [b'+', rest @ ..] => (ComparisonKind::GreaterThan, rest),
        [b'-', rest @ ..] => (ComparisonKind::LessThan, rest),
        _ => (ComparisonKind::Exactly, bytes),
    };

    if digits.is_empty() || !digits.iter().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_size_argument(raw));
    }

    let parsed = str::from_utf8(digits)
        .map_err(|_| invalid_size_argument(raw))?
        .parse::<u64>()
        .map_err(|_| invalid_size_argument(raw))?;

    Ok(match kind {
        ComparisonKind::Exactly => NumericComparison::Exactly(parsed),
        ComparisonKind::LessThan => NumericComparison::LessThan(parsed),
        ComparisonKind::GreaterThan => NumericComparison::GreaterThan(parsed),
    })
}

fn rounded_up_units(size_bytes: u64, unit_bytes: u64) -> u64 {
    if size_bytes == 0 {
        0
    } else {
        1 + ((size_bytes - 1) / unit_bytes)
    }
}

fn invalid_size_argument(raw: &OsStr) -> Diagnostic {
    Diagnostic::parse(format!("invalid size argument `{}`", raw.to_string_lossy()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonKind {
    Exactly,
    LessThan,
    GreaterThan,
}
