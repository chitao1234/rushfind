use crate::ctype::CtypeProfile;
use crate::ctype::case::{chars_equal_folded, fold_char};
use crate::ctype::class::{PosixClass, class_contains};
use crate::ctype::text::{TextUnit, decode_units};
use crate::diagnostics::Diagnostic;
use crate::regex_match::RegexDialect;
use crate::regex_match::ir::{
    AnchorKind, AssertionKind, ClassExpr, ClassItem, GnuExpr, GnuRegex, RepetitionKind,
};

pub(crate) type LocaleGnuRegex = GnuRegex;

pub(crate) fn compile(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    ctype: &CtypeProfile,
) -> Result<LocaleGnuRegex, Diagnostic> {
    crate::regex_match::gnu::parse_gnu_regex_with_ctype(flag, dialect, pattern, ctype)
}

pub(crate) fn is_match(
    regex: &LocaleGnuRegex,
    ctype: &CtypeProfile,
    candidate: &[u8],
    case_insensitive: bool,
) -> Result<bool, Diagnostic> {
    let units = decode_units(ctype, candidate).collect::<Vec<_>>();
    Ok(match_expr(&regex.expr, &units, 0, case_insensitive)
        .into_iter()
        .any(|end| end == units.len()))
}

pub(crate) fn can_execute(expr: &GnuExpr) -> bool {
    match expr {
        GnuExpr::Empty
        | GnuExpr::Literal(_)
        | GnuExpr::LiteralChar(_)
        | GnuExpr::Dot
        | GnuExpr::Class(_)
        | GnuExpr::Anchor(_)
        | GnuExpr::Assertion(_)
        | GnuExpr::WordByteClass { .. } => true,
        GnuExpr::Backreference(_) => false,
        GnuExpr::Concat(items) | GnuExpr::Alternation(items) => items.iter().all(can_execute),
        GnuExpr::Group { expr, .. } | GnuExpr::Repeat { expr, .. } => can_execute(expr),
    }
}

fn match_expr(
    expr: &GnuExpr,
    units: &[TextUnit<'_>],
    pos: usize,
    case_insensitive: bool,
) -> Vec<usize> {
    match expr {
        GnuExpr::Empty => vec![pos],
        GnuExpr::Literal(byte) => {
            if byte.is_ascii() {
                match_literal(*byte as char, units, pos, case_insensitive)
            } else {
                Vec::new()
            }
        }
        GnuExpr::LiteralChar(ch) => match_literal(*ch, units, pos, case_insensitive),
        GnuExpr::Dot => {
            if pos < units.len() {
                vec![pos + 1]
            } else {
                Vec::new()
            }
        }
        GnuExpr::Concat(items) => {
            let mut positions = vec![pos];
            for item in items {
                let mut next = Vec::new();
                for position in positions {
                    next.extend(match_expr(item, units, position, case_insensitive));
                }
                positions = next;
                if positions.is_empty() {
                    break;
                }
            }
            positions
        }
        GnuExpr::Alternation(items) => items
            .iter()
            .flat_map(|item| match_expr(item, units, pos, case_insensitive))
            .collect(),
        GnuExpr::Group { expr, .. } => match_expr(expr, units, pos, case_insensitive),
        GnuExpr::Class(class) => match_class(class, units, pos, case_insensitive),
        GnuExpr::Repeat { expr, kind } => match_repeat(expr, *kind, units, pos, case_insensitive),
        GnuExpr::Anchor(AnchorKind::Start) => {
            if pos == 0 {
                vec![pos]
            } else {
                Vec::new()
            }
        }
        GnuExpr::Anchor(AnchorKind::End) => {
            if pos == units.len() {
                vec![pos]
            } else {
                Vec::new()
            }
        }
        GnuExpr::Backreference(_) => Vec::new(),
        GnuExpr::Assertion(kind) => match_assertion(*kind, units, pos),
        GnuExpr::WordByteClass { negated } => match_word_byte_class(*negated, units, pos),
    }
}

fn match_literal(
    expected: char,
    units: &[TextUnit<'_>],
    pos: usize,
    case_insensitive: bool,
) -> Vec<usize> {
    let Some(unit) = units.get(pos).copied() else {
        return Vec::new();
    };
    let Some(actual) = unit.as_char() else {
        return Vec::new();
    };
    if char_eq(expected, actual, case_insensitive) {
        vec![pos + 1]
    } else {
        Vec::new()
    }
}

fn match_class(
    class: &ClassExpr,
    units: &[TextUnit<'_>],
    pos: usize,
    case_insensitive: bool,
) -> Vec<usize> {
    let Some(unit) = units.get(pos).copied() else {
        return Vec::new();
    };
    let Some(actual) = unit.as_char() else {
        return Vec::new();
    };
    let matched = class.items.iter().any(|item| match *item {
        ClassItem::Byte(byte) => byte.is_ascii() && char_eq(byte as char, actual, case_insensitive),
        ClassItem::Char(ch) => char_eq(ch, actual, case_insensitive),
        ClassItem::Range(start, end) => {
            if start.is_ascii() && end.is_ascii() {
                char_range_contains(start as char, end as char, actual, case_insensitive)
            } else {
                false
            }
        }
        ClassItem::CharRange(start, end) => {
            char_range_contains(start, end, actual, case_insensitive)
        }
        ClassItem::PosixClass(name) => {
            let class = PosixClass::parse(name).unwrap();
            class_contains(class, actual)
        }
    });
    let matched = if class.negated { !matched } else { matched };
    if matched { vec![pos + 1] } else { Vec::new() }
}

fn match_repeat(
    expr: &GnuExpr,
    kind: RepetitionKind,
    units: &[TextUnit<'_>],
    pos: usize,
    case_insensitive: bool,
) -> Vec<usize> {
    let (min, max) = match kind {
        RepetitionKind::ZeroOrMore => (0, None),
        RepetitionKind::OneOrMore => (1, None),
        RepetitionKind::ZeroOrOne => (0, Some(1)),
        RepetitionKind::Bounded { min, max } => (min, max),
    };
    repeat_positions(expr, units, pos, case_insensitive, min, max, 0)
}

fn repeat_positions(
    expr: &GnuExpr,
    units: &[TextUnit<'_>],
    pos: usize,
    case_insensitive: bool,
    min: u32,
    max: Option<u32>,
    count: u32,
) -> Vec<usize> {
    let mut out = Vec::new();
    if count >= min {
        out.push(pos);
    }
    if max.is_some_and(|limit| count >= limit) {
        return out;
    }
    for next in match_expr(expr, units, pos, case_insensitive) {
        if next == pos {
            continue;
        }
        out.extend(repeat_positions(
            expr,
            units,
            next,
            case_insensitive,
            min,
            max,
            count + 1,
        ));
    }
    out
}

fn char_eq(expected: char, actual: char, case_insensitive: bool) -> bool {
    if case_insensitive {
        chars_equal_folded(expected, actual)
    } else {
        expected == actual
    }
}

fn char_range_contains(start: char, end: char, actual: char, case_insensitive: bool) -> bool {
    let actual = if case_insensitive {
        fold_char(actual)
    } else {
        actual
    };
    let start = if case_insensitive {
        fold_char(start)
    } else {
        start
    };
    let end = if case_insensitive {
        fold_char(end)
    } else {
        end
    };
    start <= actual && actual <= end
}

fn match_word_byte_class(negated: bool, units: &[TextUnit<'_>], pos: usize) -> Vec<usize> {
    let matched = units
        .get(pos)
        .copied()
        .and_then(TextUnit::as_char)
        .is_some_and(is_ascii_word);
    if matched ^ negated {
        vec![pos + 1]
    } else {
        Vec::new()
    }
}

fn is_ascii_word(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn match_assertion(kind: AssertionKind, units: &[TextUnit<'_>], pos: usize) -> Vec<usize> {
    let before = pos
        .checked_sub(1)
        .and_then(|idx| units.get(idx))
        .and_then(|unit| unit.as_char());
    let after = units.get(pos).copied().and_then(TextUnit::as_char);
    let before_word = before.is_some_and(is_ascii_word);
    let after_word = after.is_some_and(is_ascii_word);
    let matched = match kind {
        AssertionKind::WordBoundary => before_word != after_word,
        AssertionKind::NotWordBoundary => before_word == after_word,
        AssertionKind::WordStart => !before_word && after_word,
        AssertionKind::WordEnd => before_word && !after_word,
        AssertionKind::BufferStart => pos == 0,
        AssertionKind::BufferEnd => pos == units.len(),
    };
    if matched { vec![pos] } else { Vec::new() }
}
