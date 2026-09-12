use super::{GlobCaseMode, GlobSlashMode};
use crate::ctype::CtypeProfile;
use crate::ctype::case::{chars_equal_folded, fold_char};
use crate::ctype::class::{PosixClass, class_contains};
use crate::ctype::text::{TextUnit, decode_units};
use crate::diagnostics::Diagnostic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EncodedGlobProgram {
    atoms: Vec<EncodedAtom>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EncodedAtom {
    Literal(char),
    AnyChar,
    AnySequence,
    Class(EncodedClass),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EncodedClass {
    negated: bool,
    items: Vec<EncodedClassItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EncodedClassItem {
    Literal(char),
    Range(char, char),
    Posix(PosixClass),
}

pub(super) fn compile_pattern(
    flag: &str,
    pattern: &[u8],
    ctype: &CtypeProfile,
) -> Result<EncodedGlobProgram, Diagnostic> {
    let mut atoms = Vec::new();
    let units = decode_units(ctype, pattern).collect::<Vec<_>>();
    let mut idx = 0usize;

    while let Some(unit) = units.get(idx).copied() {
        idx += 1;
        match unit.as_char() {
            Some('\\') => {
                let literal = units
                    .get(idx)
                    .copied()
                    .and_then(TextUnit::as_char)
                    .unwrap_or('\\');
                if idx < units.len() {
                    idx += 1;
                }
                atoms.push(EncodedAtom::Literal(literal));
            }
            Some('*') => {
                if !matches!(atoms.last(), Some(EncodedAtom::AnySequence)) {
                    atoms.push(EncodedAtom::AnySequence);
                }
            }
            Some('?') => atoms.push(EncodedAtom::AnyChar),
            Some('[') => match try_parse_class(flag, &units, &mut idx)? {
                Some(class) => atoms.push(EncodedAtom::Class(class)),
                None => atoms.push(EncodedAtom::Literal('[')),
            },
            Some(ch) => atoms.push(EncodedAtom::Literal(ch)),
            None => {
                return Err(Diagnostic::new(
                    format!("invalid encoded character in glob pattern for `{flag}`"),
                    1,
                ));
            }
        }
    }

    Ok(EncodedGlobProgram { atoms })
}

fn try_parse_class(
    flag: &str,
    units: &[TextUnit<'_>],
    idx: &mut usize,
) -> Result<Option<EncodedClass>, Diagnostic> {
    let start_idx = *idx;
    let mut negated = false;
    if matches!(
        units.get(*idx).and_then(|unit| unit.as_char()),
        Some('!') | Some('^')
    ) {
        negated = true;
        *idx += 1;
    }

    let mut items = Vec::new();
    let mut saw_non_closing = false;
    if units.get(*idx).and_then(|unit| unit.as_char()) == Some(']') {
        items.push(EncodedClassItem::Literal(']'));
        *idx += 1;
        saw_non_closing = true;
    }

    while let Some(unit) = units.get(*idx).copied() {
        if unit.as_char() == Some(']') && saw_non_closing {
            *idx += 1;
            return Ok(Some(EncodedClass { negated, items }));
        }

        let Some(start) = consume_class_item(flag, units, idx)? else {
            *idx = start_idx;
            return Ok(None);
        };
        saw_non_closing = true;

        if units.get(*idx).and_then(|unit| unit.as_char()) == Some('-')
            && units.get(*idx + 1).and_then(|unit| unit.as_char()) != Some(']')
        {
            *idx += 1;
            let Some(end) = consume_class_item(flag, units, idx)? else {
                *idx = start_idx;
                return Ok(None);
            };
            match (start, end) {
                (EncodedClassItem::Literal(start), EncodedClassItem::Literal(end)) => {
                    items.push(EncodedClassItem::Range(start, end));
                }
                (start, end) => {
                    items.push(start);
                    items.push(EncodedClassItem::Literal('-'));
                    items.push(end);
                }
            }
        } else {
            items.push(start);
        }
    }

    *idx = start_idx;
    Ok(None)
}

fn consume_class_item(
    flag: &str,
    units: &[TextUnit<'_>],
    idx: &mut usize,
) -> Result<Option<EncodedClassItem>, Diagnostic> {
    let Some(unit) = units.get(*idx).copied() else {
        return Ok(None);
    };
    *idx += 1;

    match unit.as_char() {
        Some('[') if units.get(*idx).and_then(|unit| unit.as_char()) == Some(':') => {
            *idx += 1;
            let mut name = String::new();
            while *idx + 1 < units.len() {
                if units[*idx].as_char() == Some(':') && units[*idx + 1].as_char() == Some(']') {
                    *idx += 2;
                    let class = PosixClass::parse(&name).ok_or_else(|| {
                        Diagnostic::new(
                            format!(
                                "unsupported POSIX character class `[:{name}:]` in glob pattern for `{flag}`"
                            ),
                            1,
                        )
                    })?;
                    return Ok(Some(EncodedClassItem::Posix(class)));
                }

                let Some(ch) = units[*idx].as_char() else {
                    return Err(Diagnostic::new(
                        format!("invalid POSIX character class in glob pattern for `{flag}`"),
                        1,
                    ));
                };
                name.push(ch);
                *idx += 1;
            }
            Ok(None)
        }
        Some('[') if units.get(*idx).and_then(|unit| unit.as_char()) == Some('.') => {
            Err(Diagnostic::new(
                format!(
                    "unsupported construct in glob pattern for `{flag}`: POSIX collating symbols are out of scope"
                ),
                1,
            ))
        }
        Some('[') if units.get(*idx).and_then(|unit| unit.as_char()) == Some('=') => {
            Err(Diagnostic::new(
                format!(
                    "unsupported construct in glob pattern for `{flag}`: POSIX equivalence classes are out of scope"
                ),
                1,
            ))
        }
        Some('\\') => {
            let literal = units
                .get(*idx)
                .copied()
                .and_then(TextUnit::as_char)
                .unwrap_or('\\');
            if *idx < units.len() {
                *idx += 1;
            }
            Ok(Some(EncodedClassItem::Literal(literal)))
        }
        Some(ch) => Ok(Some(EncodedClassItem::Literal(ch))),
        None => Err(Diagnostic::new(
            format!("invalid encoded character in glob pattern for `{flag}`"),
            1,
        )),
    }
}

pub(super) fn matches(
    program: &EncodedGlobProgram,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    ctype: &CtypeProfile,
    candidate: &[u8],
) -> bool {
    let units = decode_units(ctype, candidate).collect::<Vec<_>>();
    matches_from(program, case_mode, slash_mode, &units, 0, 0)
}

fn matches_from(
    program: &EncodedGlobProgram,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    candidate: &[TextUnit<'_>],
    atom_idx: usize,
    cand_idx: usize,
) -> bool {
    if atom_idx == program.atoms.len() {
        return cand_idx == candidate.len();
    }

    match &program.atoms[atom_idx] {
        EncodedAtom::Literal(expected) => candidate.get(cand_idx).is_some_and(|actual| {
            unit_matches_literal(*expected, *actual, case_mode)
                && matches_from(
                    program,
                    case_mode,
                    slash_mode,
                    candidate,
                    atom_idx + 1,
                    cand_idx + 1,
                )
        }),
        EncodedAtom::AnyChar => candidate.get(cand_idx).is_some_and(|actual| {
            (!actual.is_slash() || slash_mode == GlobSlashMode::Literal)
                && matches_from(
                    program,
                    case_mode,
                    slash_mode,
                    candidate,
                    atom_idx + 1,
                    cand_idx + 1,
                )
        }),
        EncodedAtom::AnySequence => {
            if matches_from(
                program,
                case_mode,
                slash_mode,
                candidate,
                atom_idx + 1,
                cand_idx,
            ) {
                return true;
            }

            let mut idx = cand_idx;
            while let Some(unit) = candidate.get(idx).copied() {
                if slash_mode == GlobSlashMode::Pathname && unit.is_slash() {
                    break;
                }
                idx += 1;
                if matches_from(program, case_mode, slash_mode, candidate, atom_idx + 1, idx) {
                    return true;
                }
            }
            false
        }
        EncodedAtom::Class(class) => candidate.get(cand_idx).is_some_and(|actual| {
            (!actual.is_slash() || slash_mode == GlobSlashMode::Literal)
                && class_matches(class, *actual, case_mode)
                && matches_from(
                    program,
                    case_mode,
                    slash_mode,
                    candidate,
                    atom_idx + 1,
                    cand_idx + 1,
                )
        }),
    }
}

fn unit_matches_literal(expected: char, actual: TextUnit<'_>, case_mode: GlobCaseMode) -> bool {
    let Some(ch) = actual.as_char() else {
        return false;
    };
    chars_match(expected, ch, case_mode)
}

fn class_matches(class: &EncodedClass, actual: TextUnit<'_>, case_mode: GlobCaseMode) -> bool {
    let Some(ch) = actual.as_char() else {
        return class.negated;
    };

    let matched = class.items.iter().any(|item| match *item {
        EncodedClassItem::Literal(expected) => chars_match(expected, ch, case_mode),
        EncodedClassItem::Range(start, end) => {
            let folded = fold_for_class(ch, case_mode);
            let folded_start = fold_for_class(start, case_mode);
            let folded_end = fold_for_class(end, case_mode);
            folded_start <= folded && folded <= folded_end
        }
        EncodedClassItem::Posix(posix) => class_contains(posix, ch),
    });

    if class.negated { !matched } else { matched }
}

fn chars_match(expected: char, actual: char, case_mode: GlobCaseMode) -> bool {
    match case_mode {
        GlobCaseMode::Sensitive => expected == actual,
        GlobCaseMode::Insensitive => chars_equal_folded(expected, actual),
    }
}

fn fold_for_class(ch: char, case_mode: GlobCaseMode) -> char {
    match case_mode {
        GlobCaseMode::Sensitive => ch,
        GlobCaseMode::Insensitive => fold_char(ch),
    }
}
