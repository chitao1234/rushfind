use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermMatcher {
    Exact(u32),
    All(u32),
    Any(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolicClause {
    pub who: WhoMask,
    pub op: SymbolicOp,
    pub perms: Vec<SymbolicPerm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhoMask(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolicOp {
    Add,
    Remove,
    Assign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolicPerm {
    Read,
    Write,
    Execute,
    ConditionalExecute,
    SetId,
    Sticky,
    CopyUser,
    CopyGroup,
    CopyOther,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermMatchKind {
    Exact,
    All,
    Any,
}

pub fn parse_perm_argument(raw: &OsStr) -> Result<PermMatcher, Diagnostic> {
    let rendered = raw.to_string_lossy();
    let bytes = rendered.as_bytes();
    let (kind, body) = match bytes {
        [b'-', rest @ ..] => (PermMatchKind::All, rest),
        [b'/', rest @ ..] => (PermMatchKind::Any, rest),
        [b'+', rest @ ..] if rest.iter().all(|byte| byte.is_ascii_digit()) => {
            return Err(invalid_mode(raw));
        }
        _ => (PermMatchKind::Exact, bytes),
    };

    if body.is_empty() {
        return Err(invalid_mode(raw));
    }

    let mask = if body.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
        parse_octal_mask(body, raw)?
    } else {
        let symbolic = std::str::from_utf8(body).map_err(|_| invalid_mode(raw))?;
        resolve_symbolic_mode(symbolic, kind, raw)?
    };

    Ok(match kind {
        PermMatchKind::Exact => PermMatcher::Exact(mask),
        PermMatchKind::All => PermMatcher::All(mask),
        PermMatchKind::Any => PermMatcher::Any(mask),
    })
}

impl PermMatcher {
    pub fn matches(&self, actual_mode: u32) -> bool {
        let actual = actual_mode & 0o7777;
        match self {
            Self::Exact(expected) => *expected == actual,
            Self::All(required) => (actual & required) == *required,
            Self::Any(required) => *required == 0 || (actual & required) != 0,
        }
    }
}

fn parse_octal_mask(bytes: &[u8], raw: &OsStr) -> Result<u32, Diagnostic> {
    let rendered = std::str::from_utf8(bytes).map_err(|_| invalid_mode(raw))?;
    u32::from_str_radix(rendered, 8)
        .map(|value| value & 0o7777)
        .map_err(|_| invalid_mode(raw))
}

fn resolve_symbolic_mode(
    symbolic: &str,
    kind: PermMatchKind,
    raw: &OsStr,
) -> Result<u32, Diagnostic> {
    let clauses = parse_symbolic_clauses(symbolic, kind, raw)?;
    let mut mode = 0;

    for clause in &clauses {
        let resolved = resolve_symbolic_perms(clause, mode);
        mode = match clause.op {
            SymbolicOp::Add => mode | resolved,
            SymbolicOp::Remove => mode & !resolved,
            SymbolicOp::Assign => (mode & !assign_target_bits(clause.who)) | resolved,
        };
    }

    Ok(mode & 0o7777)
}

fn parse_symbolic_clauses(
    symbolic: &str,
    kind: PermMatchKind,
    raw: &OsStr,
) -> Result<Vec<SymbolicClause>, Diagnostic> {
    let mut clauses = Vec::new();
    for clause in symbolic.split(',') {
        clauses.push(parse_symbolic_clause(clause, kind, raw)?);
    }
    if clauses.is_empty() {
        return Err(invalid_mode(raw));
    }
    Ok(clauses)
}

fn parse_symbolic_clause(
    clause: &str,
    kind: PermMatchKind,
    raw: &OsStr,
) -> Result<SymbolicClause, Diagnostic> {
    let bytes = clause.as_bytes();
    let mut index = 0;
    let mut who = WhoMask(0);

    while let Some(byte) = bytes.get(index) {
        match byte {
            b'u' => who.0 |= 0b001,
            b'g' => who.0 |= 0b010,
            b'o' => who.0 |= 0b100,
            b'a' => who.0 |= 0b111,
            _ => break,
        }
        index += 1;
    }

    if who.0 == 0 {
        if matches!(kind, PermMatchKind::All | PermMatchKind::Any) {
            return Err(invalid_mode(raw));
        }
        who = WhoMask(0b111);
    }

    let op = match bytes.get(index) {
        Some(b'+') => SymbolicOp::Add,
        Some(b'-') => SymbolicOp::Remove,
        Some(b'=') => SymbolicOp::Assign,
        _ => return Err(invalid_mode(raw)),
    };
    index += 1;

    let mut perms = Vec::new();
    while let Some(byte) = bytes.get(index) {
        perms.push(match byte {
            b'r' => SymbolicPerm::Read,
            b'w' => SymbolicPerm::Write,
            b'x' => SymbolicPerm::Execute,
            b'X' => SymbolicPerm::ConditionalExecute,
            b's' => SymbolicPerm::SetId,
            b't' => SymbolicPerm::Sticky,
            b'u' => SymbolicPerm::CopyUser,
            b'g' => SymbolicPerm::CopyGroup,
            b'o' => SymbolicPerm::CopyOther,
            _ => return Err(invalid_mode(raw)),
        });
        index += 1;
    }

    Ok(SymbolicClause { who, op, perms })
}

fn resolve_symbolic_perms(clause: &SymbolicClause, current_mode: u32) -> u32 {
    let mut bits = 0;
    for perm in &clause.perms {
        bits |= match perm {
            SymbolicPerm::Read => class_bits(clause.who, 0o444),
            SymbolicPerm::Write => class_bits(clause.who, 0o222),
            SymbolicPerm::Execute => class_bits(clause.who, 0o111),
            SymbolicPerm::ConditionalExecute => {
                if (current_mode & 0o111) != 0 {
                    class_bits(clause.who, 0o111)
                } else {
                    0
                }
            }
            SymbolicPerm::SetId => {
                (if clause.who.0 & 0b001 != 0 { 0o4000 } else { 0 })
                    | (if clause.who.0 & 0b010 != 0 { 0o2000 } else { 0 })
            }
            SymbolicPerm::Sticky => {
                if clause.who.0 & 0b100 != 0 {
                    0o1000
                } else {
                    0
                }
            }
            SymbolicPerm::CopyUser => copy_class_bits(current_mode, clause.who, 0o700),
            SymbolicPerm::CopyGroup => copy_class_bits(current_mode, clause.who, 0o070),
            SymbolicPerm::CopyOther => copy_class_bits(current_mode, clause.who, 0o007),
        };
    }
    bits
}

fn assign_target_bits(who: WhoMask) -> u32 {
    (if who.0 & 0b001 != 0 { 0o4700 } else { 0 })
        | (if who.0 & 0b010 != 0 { 0o2070 } else { 0 })
        | (if who.0 & 0b100 != 0 { 0o1007 } else { 0 })
}

fn class_bits(who: WhoMask, template: u32) -> u32 {
    (if who.0 & 0b001 != 0 { template & 0o700 } else { 0 })
        | (if who.0 & 0b010 != 0 { template & 0o070 } else { 0 })
        | (if who.0 & 0b100 != 0 { template & 0o007 } else { 0 })
}

fn copy_class_bits(current_mode: u32, who: WhoMask, source_mask: u32) -> u32 {
    let source = match source_mask {
        0o700 => (current_mode & 0o700) >> 6,
        0o070 => (current_mode & 0o070) >> 3,
        0o007 => current_mode & 0o007,
        _ => 0,
    };

    (if who.0 & 0b001 != 0 { source << 6 } else { 0 })
        | (if who.0 & 0b010 != 0 { source << 3 } else { 0 })
        | (if who.0 & 0b100 != 0 { source } else { 0 })
}

fn invalid_mode(raw: &OsStr) -> Diagnostic {
    Diagnostic::new(format!("invalid mode `{}`", raw.to_string_lossy()), 1)
}
