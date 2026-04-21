use super::ir::{ClassItem, GlobAtom, GlobClass, GlobProgram};
use super::{GlobCaseMode, GlobSlashMode};
use crate::diagnostics::Diagnostic;

pub(super) fn matches(
    program: &GlobProgram,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    candidate: &[u8],
) -> Result<bool, Diagnostic> {
    Ok(matches_from(
        program, case_mode, slash_mode, candidate, 0, 0,
    ))
}

fn matches_from(
    program: &GlobProgram,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    candidate: &[u8],
    atom_idx: usize,
    cand_idx: usize,
) -> bool {
    if atom_idx == program.len() {
        return cand_idx == candidate.len();
    }

    match &program[atom_idx] {
        GlobAtom::Literal(expected) => candidate.get(cand_idx).is_some_and(|actual| {
            eq_byte(*expected, *actual, case_mode)
                && matches_from(
                    program,
                    case_mode,
                    slash_mode,
                    candidate,
                    atom_idx + 1,
                    cand_idx + 1,
                )
        }),
        GlobAtom::AnyByte => candidate.get(cand_idx).is_some_and(|actual| {
            (*actual != b'/' || slash_mode == GlobSlashMode::Literal)
                && matches_from(
                    program,
                    case_mode,
                    slash_mode,
                    candidate,
                    atom_idx + 1,
                    cand_idx + 1,
                )
        }),
        GlobAtom::AnySequence => {
            let mut idx = cand_idx;
            if matches_from(program, case_mode, slash_mode, candidate, atom_idx + 1, idx) {
                return true;
            }
            while let Some(&actual) = candidate.get(idx) {
                if slash_mode == GlobSlashMode::Pathname && actual == b'/' {
                    break;
                }
                idx += 1;
                if matches_from(program, case_mode, slash_mode, candidate, atom_idx + 1, idx) {
                    return true;
                }
            }
            false
        }
        GlobAtom::Class(class) => candidate.get(cand_idx).is_some_and(|actual| {
            (*actual != b'/' || slash_mode == GlobSlashMode::Literal)
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

fn class_matches(class: &GlobClass, actual: u8, case_mode: GlobCaseMode) -> bool {
    let folded_actual = fold_byte(actual, case_mode);
    let matched = class.items.iter().any(|item| match *item {
        ClassItem::Literal(expected) => fold_byte(expected, case_mode) == folded_actual,
        ClassItem::Range(start, end) => {
            let folded_start = fold_byte(start, case_mode);
            let folded_end = fold_byte(end, case_mode);
            folded_start <= folded_actual && folded_actual <= folded_end
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
