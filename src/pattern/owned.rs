use super::ir::{ClassItem, GlobAtom, GlobClass, GlobProgram};
use super::{GlobCaseMode, GlobSlashMode};
use crate::diagnostics::Diagnostic;

pub(super) fn matches(
    program: &GlobProgram,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    candidate: &[u8],
) -> Result<bool, Diagnostic> {
    let mut pattern_idx = 0usize;
    let mut candidate_idx = 0usize;
    let mut star_pattern_idx = None;
    let mut star_candidate_idx = 0usize;

    while candidate_idx < candidate.len() {
        if let Some(atom) = program.get(pattern_idx) {
            if !matches!(atom, GlobAtom::AnySequence)
                && atom_matches(atom, candidate[candidate_idx], case_mode, slash_mode)
            {
                pattern_idx += 1;
                candidate_idx += 1;
                continue;
            }
            if matches!(atom, GlobAtom::AnySequence) {
                star_pattern_idx = Some(pattern_idx);
                star_candidate_idx = candidate_idx;
                pattern_idx += 1;
                continue;
            }
        }

        let Some(star_idx) = star_pattern_idx else {
            return Ok(false);
        };
        if star_candidate_idx >= candidate.len()
            || (slash_mode == GlobSlashMode::Pathname && candidate[star_candidate_idx] == b'/')
        {
            return Ok(false);
        }
        star_candidate_idx += 1;
        candidate_idx = star_candidate_idx;
        pattern_idx = star_idx + 1;
    }

    while matches!(program.get(pattern_idx), Some(GlobAtom::AnySequence)) {
        pattern_idx += 1;
    }
    Ok(pattern_idx == program.len())
}

fn atom_matches(
    atom: &GlobAtom,
    actual: u8,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
) -> bool {
    match atom {
        GlobAtom::Literal(expected) => eq_byte(*expected, actual, case_mode),
        GlobAtom::AnyByte => slash_mode == GlobSlashMode::Literal || actual != b'/',
        GlobAtom::Class(class) => {
            (slash_mode == GlobSlashMode::Literal || actual != b'/')
                && class_matches(class, actual, case_mode)
        }
        GlobAtom::AnySequence => false,
    }
}

fn class_matches(class: &GlobClass, actual: u8, case_mode: GlobCaseMode) -> bool {
    let folded_actual = fold_byte(actual, case_mode);
    let matched = class.items.iter().any(|item| match *item {
        ClassItem::Literal(expected) => fold_byte(expected, case_mode) == folded_actual,
        ClassItem::Range(start, end) => {
            let folded_start = fold_byte(start, case_mode);
            let folded_end = fold_byte(end, case_mode);
            folded_start <= folded_actual && folded_actual <= folded_end
        }
        ClassItem::Posix(class) => {
            actual.is_ascii() && crate::ctype::class::class_contains(class, actual as char)
        }
    });

    if class.negated { !matched } else { matched }
}

fn eq_byte(expected: u8, actual: u8, case_mode: GlobCaseMode) -> bool {
    fold_byte(expected, case_mode) == fold_byte(actual, case_mode)
}

fn fold_byte(byte: u8, case_mode: GlobCaseMode) -> u8 {
    match case_mode {
        GlobCaseMode::Sensitive => byte,
        GlobCaseMode::Insensitive => byte.to_ascii_lowercase(),
    }
}
