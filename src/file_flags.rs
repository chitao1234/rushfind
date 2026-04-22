use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagMatchMode {
    Exact,
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagSpec {
    pub name: &'static str,
    pub bit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagCondition {
    pub bit: u64,
    pub must_be_set: bool,
}

impl FlagCondition {
    pub fn set(bit: u64) -> Self {
        Self {
            bit,
            must_be_set: true,
        }
    }

    pub fn clear(bit: u64) -> Self {
        Self {
            bit,
            must_be_set: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFlagsMatcher {
    universe_mask: u64,
    mode: FlagMatchMode,
    conditions: Arc<[FlagCondition]>,
}

impl FileFlagsMatcher {
    pub fn new(mode: FlagMatchMode, universe_mask: u64, conditions: Vec<FlagCondition>) -> Self {
        Self {
            universe_mask,
            mode,
            conditions: conditions.into(),
        }
    }

    pub fn matches(&self, observed: Option<u64>) -> bool {
        let Some(bits) = observed else {
            return false;
        };

        match self.mode {
            FlagMatchMode::Exact => {
                let expected = self
                    .conditions
                    .iter()
                    .filter(|condition| condition.must_be_set)
                    .fold(0u64, |mask, condition| mask | condition.bit);
                (bits & self.universe_mask) == expected
            }
            FlagMatchMode::All => self
                .conditions
                .iter()
                .all(|condition| ((bits & condition.bit) != 0) == condition.must_be_set),
            FlagMatchMode::Any => self
                .conditions
                .iter()
                .any(|condition| ((bits & condition.bit) != 0) == condition.must_be_set),
        }
    }
}

pub fn parse_flags_argument(
    raw: &OsStr,
    specs: &'static [FlagSpec],
) -> Result<FileFlagsMatcher, Diagnostic> {
    let text = raw.to_string_lossy();
    let (mode, body) = match text.chars().next() {
        Some('+') => (FlagMatchMode::Any, &text[1..]),
        Some('-') => (FlagMatchMode::All, &text[1..]),
        _ => (FlagMatchMode::Exact, text.as_ref()),
    };

    if body.is_empty() {
        return Err(Diagnostic::new(
            "-flags requires a non-empty symbolic operand",
            1,
        ));
    }

    let universe_mask = specs.iter().fold(0u64, |mask, spec| mask | spec.bit);
    let mut conditions = Vec::new();

    for token in body.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(Diagnostic::new("unknown -flags name ``", 1));
        }

        if let Some(spec) = specs.iter().find(|spec| spec.name == token) {
            push_condition(&mut conditions, spec.bit, true, spec.name)?;
            continue;
        }

        let (name, must_be_set) = if token != "nodump" && token.starts_with("no") {
            (&token[2..], false)
        } else {
            (token, true)
        };

        let spec = specs
            .iter()
            .find(|spec| spec.name == name)
            .ok_or_else(|| Diagnostic::new(format!("unknown -flags name `{token}`"), 1))?;
        push_condition(&mut conditions, spec.bit, must_be_set, name)?;
    }

    Ok(FileFlagsMatcher::new(mode, universe_mask, conditions))
}

fn push_condition(
    conditions: &mut Vec<FlagCondition>,
    bit: u64,
    must_be_set: bool,
    label: &str,
) -> Result<(), Diagnostic> {
    if conditions
        .iter()
        .any(|condition| condition.bit == bit && condition.must_be_set != must_be_set)
    {
        return Err(Diagnostic::new(
            format!("contradictory -flags conditions for `{label}`"),
            1,
        ));
    }

    if !conditions
        .iter()
        .any(|condition| condition.bit == bit && condition.must_be_set == must_be_set)
    {
        conditions.push(FlagCondition { bit, must_be_set });
    }

    Ok(())
}
