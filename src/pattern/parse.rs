use super::ir::{ClassItem, GlobAtom, GlobClass, GlobProgram};
use super::{GlobCaseMode, GlobSlashMode};
use crate::diagnostics::Diagnostic;

pub(super) struct ParsedGlob {
    pub(super) program: GlobProgram,
    pub(super) contains_bracket_expr: bool,
}

pub(super) fn compile_pattern(
    flag: &str,
    pattern: &[u8],
    _case_mode: GlobCaseMode,
    _slash_mode: GlobSlashMode,
) -> Result<ParsedGlob, Diagnostic> {
    let mut program = Vec::new();
    let mut idx = 0usize;
    let mut contains_bracket_expr = false;

    while let Some(&byte) = pattern.get(idx) {
        idx += 1;
        match byte {
            b'\\' => {
                let literal = pattern.get(idx).copied().unwrap_or(byte);
                if idx < pattern.len() {
                    idx += 1;
                }
                program.push(GlobAtom::Literal(literal));
            }
            b'*' => {
                if !matches!(program.last(), Some(GlobAtom::AnySequence)) {
                    program.push(GlobAtom::AnySequence);
                }
            }
            b'?' => program.push(GlobAtom::AnyByte),
            b'[' => match try_parse_class(flag, pattern, idx)? {
                Some((class, next)) => {
                    idx = next;
                    contains_bracket_expr = true;
                    program.push(GlobAtom::Class(class));
                }
                None => program.push(GlobAtom::Literal(byte)),
            },
            other => program.push(GlobAtom::Literal(other)),
        }
    }

    Ok(ParsedGlob {
        program,
        contains_bracket_expr,
    })
}

fn try_parse_class(
    flag: &str,
    pattern: &[u8],
    mut idx: usize,
) -> Result<Option<(GlobClass, usize)>, Diagnostic> {
    let mut negated = false;
    if matches!(pattern.get(idx), Some(b'!') | Some(b'^')) {
        negated = true;
        idx += 1;
    }

    let mut items = Vec::new();
    let mut saw_non_closing = false;

    if matches!(pattern.get(idx), Some(b']')) {
        items.push(ClassItem::Literal(b']'));
        idx += 1;
        saw_non_closing = true;
    }

    while idx < pattern.len() {
        if pattern[idx] == b']' && saw_non_closing {
            return Ok(Some((GlobClass { negated, items }, idx + 1)));
        }

        let Some(start) = consume_class_byte(flag, pattern, &mut idx)? else {
            return Ok(None);
        };
        saw_non_closing = true;

        if matches!(pattern.get(idx), Some(b'-'))
            && !matches!(pattern.get(idx + 1), None | Some(b']'))
        {
            idx += 1;
            let Some(end) = consume_class_byte(flag, pattern, &mut idx)? else {
                return Ok(None);
            };
            items.push(ClassItem::Range(start, end));
        } else {
            items.push(ClassItem::Literal(start));
        }
    }

    Ok(None)
}

fn consume_class_byte(
    flag: &str,
    pattern: &[u8],
    idx: &mut usize,
) -> Result<Option<u8>, Diagnostic> {
    let Some(&byte) = pattern.get(*idx) else {
        return Ok(None);
    };
    *idx += 1;
    if byte == b'[' {
        match pattern.get(*idx).copied() {
            Some(b'.') => {
                return Err(Diagnostic::new(
                    format!(
                        "unsupported construct in glob pattern for `{flag}`: POSIX collating symbols are out of scope"
                    ),
                    1,
                ));
            }
            Some(b'=') => {
                return Err(Diagnostic::new(
                    format!(
                        "unsupported construct in glob pattern for `{flag}`: POSIX equivalence classes are out of scope"
                    ),
                    1,
                ));
            }
            _ => {}
        }
    }
    if byte == b'\\' {
        if let Some(&escaped) = pattern.get(*idx) {
            *idx += 1;
            Ok(Some(escaped))
        } else {
            Ok(Some(byte))
        }
    } else {
        Ok(Some(byte))
    }
}
