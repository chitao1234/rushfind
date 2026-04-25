use super::ir::{ClassItem, GlobAtom, GlobClass, GlobProgram};
use super::{GlobCaseMode, GlobSlashMode};
use crate::diagnostics::Diagnostic;

pub(super) struct ParsedGlob {
    pub(super) program: GlobProgram,
}

pub(super) fn compile_pattern(
    flag: &str,
    pattern: &[u8],
    _case_mode: GlobCaseMode,
    _slash_mode: GlobSlashMode,
) -> Result<ParsedGlob, Diagnostic> {
    let mut program = Vec::new();
    let mut idx = 0usize;

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
                    program.push(GlobAtom::Class(class));
                }
                None => program.push(GlobAtom::Literal(byte)),
            },
            other => program.push(GlobAtom::Literal(other)),
        }
    }

    Ok(ParsedGlob { program })
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

        if let Some(item) = try_parse_posix_class(flag, pattern, &mut idx)? {
            saw_non_closing = true;
            if matches!(pattern.get(idx), Some(b'-'))
                && !matches!(pattern.get(idx + 1), None | Some(b']'))
            {
                idx += 1;
                let Some(end) = consume_class_item(flag, pattern, &mut idx)? else {
                    return Ok(None);
                };
                items.push(item);
                items.push(ClassItem::Literal(b'-'));
                items.push(end);
            } else {
                items.push(item);
            }
            continue;
        }

        let Some(start) = consume_class_byte(flag, pattern, &mut idx)? else {
            return Ok(None);
        };
        saw_non_closing = true;

        if matches!(pattern.get(idx), Some(b'-'))
            && !matches!(pattern.get(idx + 1), None | Some(b']'))
        {
            idx += 1;
            let Some(end) = consume_class_item(flag, pattern, &mut idx)? else {
                return Ok(None);
            };
            if let ClassItem::Literal(end) = end {
                items.push(ClassItem::Range(start, end));
            } else {
                items.push(ClassItem::Literal(start));
                items.push(ClassItem::Literal(b'-'));
                items.push(end);
            }
        } else {
            items.push(ClassItem::Literal(start));
        }
    }

    Ok(None)
}

fn consume_class_item(
    flag: &str,
    pattern: &[u8],
    idx: &mut usize,
) -> Result<Option<ClassItem>, Diagnostic> {
    if let Some(item) = try_parse_posix_class(flag, pattern, idx)? {
        Ok(Some(item))
    } else {
        Ok(consume_class_byte(flag, pattern, idx)?.map(ClassItem::Literal))
    }
}

fn try_parse_posix_class(
    flag: &str,
    pattern: &[u8],
    idx: &mut usize,
) -> Result<Option<ClassItem>, Diagnostic> {
    if pattern.get(*idx) != Some(&b'[') || pattern.get(*idx + 1) != Some(&b':') {
        return Ok(None);
    }

    let name_start = *idx + 2;
    let mut name_end = name_start;
    while name_end + 1 < pattern.len() {
        if pattern[name_end] == b':' && pattern[name_end + 1] == b']' {
            let name = std::str::from_utf8(&pattern[name_start..name_end]).map_err(|_| {
                Diagnostic::new(
                    format!("unsupported POSIX character class in glob pattern for `{flag}`"),
                    1,
                )
            })?;
            let class = crate::ctype::class::PosixClass::parse(name).ok_or_else(|| {
                Diagnostic::new(
                    format!(
                        "unsupported POSIX character class `[:{name}:]` in glob pattern for `{flag}`"
                    ),
                    1,
                )
            })?;
            *idx = name_end + 2;
            return Ok(Some(ClassItem::Posix(class)));
        }
        name_end += 1;
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
