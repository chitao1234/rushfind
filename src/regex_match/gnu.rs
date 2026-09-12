use super::RegexDialect;
use super::backend::{
    CompiledRegex, RegexBackendKind, compile_pcre2_anchored, compile_rust_anchored,
};
use super::ir::{
    AnchorKind, AssertionKind, ClassExpr, ClassItem, GnuExpr, GnuRegex, RepetitionKind,
};
use crate::ctype::CtypeProfile;
use crate::ctype::text::{TextUnit, decode_units};
use crate::diagnostics::Diagnostic;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
enum GnuToken {
    Literal(u8),
    LiteralChar(char),
    Dot,
    AnchorStart,
    AnchorEnd,
    Class(ClassExpr),
    GroupOpen,
    GroupClose,
    Alternation,
    Quantifier(RepetitionKind),
    Backreference(u16),
    Assertion(AssertionKind),
    WordByteClass { negated: bool },
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledGnuRegex {
    pub(crate) expr: GnuRegex,
    pub backend: RegexBackendKind,
    pub translated_pattern: String,
    pub compiled: CompiledRegex,
}

pub fn compile_gnu_regex(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    case_insensitive: bool,
) -> Result<CompiledGnuRegex, Diagnostic> {
    let expr = parse_gnu_regex(flag, dialect, pattern)?;
    let backend = choose_backend(&expr);
    let translated_pattern = match backend {
        RegexBackendKind::Rust => lower_to_rust(&expr.expr)?,
        RegexBackendKind::Pcre2 => lower_to_pcre2(&expr.expr)?,
    };
    let anchored_pattern = format!(r"\A(?:{})\z", translated_pattern);
    let compiled = match backend {
        RegexBackendKind::Rust => {
            compile_rust_anchored(flag, dialect.label(), &anchored_pattern, case_insensitive)?
        }
        RegexBackendKind::Pcre2 => {
            compile_pcre2_anchored(flag, &anchored_pattern, case_insensitive)?
        }
    };

    Ok(CompiledGnuRegex {
        expr,
        backend,
        translated_pattern,
        compiled,
    })
}

pub(crate) fn compile_gnu_regex_with_ctype(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    ctype: &CtypeProfile,
) -> Result<crate::regex_match::locale::LocaleGnuRegex, Diagnostic> {
    crate::regex_match::locale::compile(flag, dialect, pattern, ctype)
}

pub fn choose_backend(expr: &GnuRegex) -> RegexBackendKind {
    fn visit(node: &GnuExpr) -> RegexBackendKind {
        match node {
            GnuExpr::Backreference(_) | GnuExpr::Assertion(_) => RegexBackendKind::Pcre2,
            GnuExpr::Empty
            | GnuExpr::Literal(_)
            | GnuExpr::LiteralChar(_)
            | GnuExpr::Dot
            | GnuExpr::Class(_)
            | GnuExpr::Anchor(_)
            | GnuExpr::WordByteClass { .. } => RegexBackendKind::Rust,
            GnuExpr::Concat(items) | GnuExpr::Alternation(items) => items
                .iter()
                .map(visit)
                .find(|backend| *backend == RegexBackendKind::Pcre2)
                .unwrap_or(RegexBackendKind::Rust),
            GnuExpr::Group { expr, .. } | GnuExpr::Repeat { expr, .. } => visit(expr),
        }
    }

    visit(&expr.expr)
}

pub fn parse_gnu_regex(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
) -> Result<GnuRegex, Diagnostic> {
    let tokens = lex_gnu_tokens(flag, dialect, pattern)?;
    let regex = TokenParser::new(flag, dialect, &tokens).parse()?;
    validate_gnu_regex(flag, dialect, &regex)?;
    Ok(regex)
}

pub(crate) fn parse_gnu_regex_with_ctype(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    ctype: &CtypeProfile,
) -> Result<GnuRegex, Diagnostic> {
    let tokens = lex_gnu_tokens_with_ctype(flag, dialect, pattern, ctype)?;
    let regex = TokenParser::new(flag, dialect, &tokens).parse()?;
    validate_gnu_regex(flag, dialect, &regex)?;
    Ok(regex)
}

fn validate_gnu_regex(
    flag: &str,
    dialect: RegexDialect,
    regex: &GnuRegex,
) -> Result<(), Diagnostic> {
    fn walk(
        expr: &GnuExpr,
        closed_captures: u16,
        flag: &str,
        dialect: RegexDialect,
    ) -> Result<u16, Diagnostic> {
        match expr {
            GnuExpr::Empty
            | GnuExpr::Literal(_)
            | GnuExpr::LiteralChar(_)
            | GnuExpr::Dot
            | GnuExpr::Class(_)
            | GnuExpr::Anchor(_)
            | GnuExpr::Assertion(_)
            | GnuExpr::WordByteClass { .. } => Ok(closed_captures),
            GnuExpr::Backreference(index) if *index == 0 || *index > closed_captures => {
                Err(malformed_regex(flag, dialect, "invalid back reference"))
            }
            GnuExpr::Backreference(_) => Ok(closed_captures),
            GnuExpr::Concat(items) => {
                let mut closed = closed_captures;
                for item in items {
                    closed = walk(item, closed, flag, dialect)?;
                }
                Ok(closed)
            }
            GnuExpr::Alternation(items) => {
                let mut max_closed = closed_captures;
                for item in items {
                    max_closed = max_closed.max(walk(item, closed_captures, flag, dialect)?);
                }
                Ok(max_closed)
            }
            GnuExpr::Group {
                capture_index,
                expr,
            } => {
                let closed = walk(expr, closed_captures, flag, dialect)?;
                Ok(closed.max(*capture_index))
            }
            GnuExpr::Repeat { expr, kind } => {
                if let RepetitionKind::Bounded {
                    min,
                    max: Some(max),
                } = kind
                {
                    if min > max {
                        return Err(malformed_regex(flag, dialect, "invalid bounded repetition"));
                    }
                }
                walk(expr, closed_captures, flag, dialect)
            }
        }
    }

    let _ = walk(&regex.expr, 0, flag, dialect)?;
    Ok(())
}

fn lex_gnu_tokens(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
) -> Result<Vec<GnuToken>, Diagnostic> {
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut can_repeat_atom = false;
    let mut group_depth = 0usize;
    let mut branch_start = true;

    while let Some(byte) = pattern.get(index).copied() {
        index += 1;
        let token = match dialect {
            RegexDialect::PosixExtended => match byte {
                b'\\' => lex_extended_escape(flag, dialect, pattern, &mut index)?,
                b'(' => {
                    group_depth += 1;
                    GnuToken::GroupOpen
                }
                b')' => {
                    if group_depth == 0 {
                        GnuToken::Literal(b')')
                    } else {
                        group_depth -= 1;
                        GnuToken::GroupClose
                    }
                }
                b'|' => GnuToken::Alternation,
                b'*' => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                b'+' => GnuToken::Quantifier(RepetitionKind::OneOrMore),
                b'?' => GnuToken::Quantifier(RepetitionKind::ZeroOrOne),
                b'{' if can_repeat_atom => {
                    GnuToken::Quantifier(lex_extended_bound(flag, dialect, pattern, &mut index)?)
                }
                b'[' => GnuToken::Class(lex_class(flag, dialect, pattern, &mut index)?),
                b'.' => GnuToken::Dot,
                b'^' => GnuToken::AnchorStart,
                b'$' => GnuToken::AnchorEnd,
                other => GnuToken::Literal(other),
            },
            RegexDialect::PosixBasic => match byte {
                b'\\' => lex_bre_or_emacs_escape(
                    flag,
                    dialect,
                    pattern,
                    &mut index,
                    can_repeat_atom,
                    &mut group_depth,
                )?,
                b'*' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                b'[' => GnuToken::Class(lex_class(flag, dialect, pattern, &mut index)?),
                b'.' => GnuToken::Dot,
                b'^' => GnuToken::AnchorStart,
                b'$' => GnuToken::AnchorEnd,
                other => GnuToken::Literal(other),
            },
            RegexDialect::Emacs => match byte {
                b'\\' => lex_bre_or_emacs_escape(
                    flag,
                    dialect,
                    pattern,
                    &mut index,
                    can_repeat_atom,
                    &mut group_depth,
                )?,
                b'^' if branch_start => GnuToken::AnchorStart,
                b'$' if emacs_dollar_is_anchor(pattern, index) => GnuToken::AnchorEnd,
                b'*' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                b'+' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::OneOrMore),
                b'?' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrOne),
                b'[' => GnuToken::Class(lex_class(flag, dialect, pattern, &mut index)?),
                b'.' => GnuToken::Dot,
                other => GnuToken::Literal(other),
            },
            RegexDialect::Rust | RegexDialect::Pcre2 => unreachable!(),
        };

        can_repeat_atom = matches!(
            token,
            GnuToken::Literal(_)
                | GnuToken::LiteralChar(_)
                | GnuToken::Dot
                | GnuToken::Class(_)
                | GnuToken::GroupClose
                | GnuToken::WordByteClass { .. }
        );
        branch_start = matches!(token, GnuToken::GroupOpen | GnuToken::Alternation);
        tokens.push(token);
    }

    if group_depth != 0 {
        return Err(malformed_regex(flag, dialect, "unclosed group"));
    }

    Ok(tokens)
}

fn lex_gnu_tokens_with_ctype(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    ctype: &CtypeProfile,
) -> Result<Vec<GnuToken>, Diagnostic> {
    let units = decode_units(ctype, pattern).collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut can_repeat_atom = false;
    let mut group_depth = 0usize;
    let mut branch_start = true;

    while let Some(unit) = units.get(index).copied() {
        index += 1;
        let ch = unit
            .as_char()
            .ok_or_else(|| malformed_regex(flag, dialect, "invalid encoded character"))?;
        let token = match dialect {
            RegexDialect::PosixExtended => match ch {
                '\\' => lex_extended_escape_units(flag, dialect, &units, &mut index)?,
                '(' => {
                    group_depth += 1;
                    GnuToken::GroupOpen
                }
                ')' => {
                    if group_depth == 0 {
                        GnuToken::Literal(b')')
                    } else {
                        group_depth -= 1;
                        GnuToken::GroupClose
                    }
                }
                '|' => GnuToken::Alternation,
                '*' => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                '+' => GnuToken::Quantifier(RepetitionKind::OneOrMore),
                '?' => GnuToken::Quantifier(RepetitionKind::ZeroOrOne),
                '{' if can_repeat_atom => GnuToken::Quantifier(lex_extended_bound_units(
                    flag, dialect, &units, &mut index,
                )?),
                '[' => GnuToken::Class(lex_class_units(flag, dialect, &units, &mut index)?),
                '.' => GnuToken::Dot,
                '^' => GnuToken::AnchorStart,
                '$' => GnuToken::AnchorEnd,
                other => literal_token_for_char(other),
            },
            RegexDialect::PosixBasic => match ch {
                '\\' => lex_bre_or_emacs_escape_units(
                    flag,
                    dialect,
                    &units,
                    &mut index,
                    can_repeat_atom,
                    &mut group_depth,
                )?,
                '*' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                '[' => GnuToken::Class(lex_class_units(flag, dialect, &units, &mut index)?),
                '.' => GnuToken::Dot,
                '^' => GnuToken::AnchorStart,
                '$' => GnuToken::AnchorEnd,
                other => literal_token_for_char(other),
            },
            RegexDialect::Emacs => match ch {
                '\\' => lex_bre_or_emacs_escape_units(
                    flag,
                    dialect,
                    &units,
                    &mut index,
                    can_repeat_atom,
                    &mut group_depth,
                )?,
                '^' if branch_start => GnuToken::AnchorStart,
                '$' if emacs_dollar_is_anchor_units(&units, index) => GnuToken::AnchorEnd,
                '*' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrMore),
                '+' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::OneOrMore),
                '?' if can_repeat_atom => GnuToken::Quantifier(RepetitionKind::ZeroOrOne),
                '[' => GnuToken::Class(lex_class_units(flag, dialect, &units, &mut index)?),
                '.' => GnuToken::Dot,
                other => literal_token_for_char(other),
            },
            RegexDialect::Rust | RegexDialect::Pcre2 => unreachable!(),
        };

        can_repeat_atom = matches!(
            token,
            GnuToken::Literal(_)
                | GnuToken::LiteralChar(_)
                | GnuToken::Dot
                | GnuToken::Class(_)
                | GnuToken::GroupClose
                | GnuToken::WordByteClass { .. }
        );
        branch_start = matches!(token, GnuToken::GroupOpen | GnuToken::Alternation);
        tokens.push(token);
    }

    if group_depth != 0 {
        return Err(malformed_regex(flag, dialect, "unclosed group"));
    }

    Ok(tokens)
}

fn literal_token_for_char(ch: char) -> GnuToken {
    if ch.is_ascii() {
        GnuToken::Literal(ch as u8)
    } else {
        GnuToken::LiteralChar(ch)
    }
}

fn lex_bre_or_emacs_escape(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
    can_repeat_atom: bool,
    group_depth: &mut usize,
) -> Result<GnuToken, Diagnostic> {
    let escaped = pattern
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "trailing `\\`"))?;
    *index += 1;

    match dialect {
        RegexDialect::PosixBasic => match escaped {
            b'1'..=b'9' => Ok(GnuToken::Backreference((escaped - b'0') as u16)),
            b'(' => {
                *group_depth += 1;
                Ok(GnuToken::GroupOpen)
            }
            b')' => {
                if *group_depth == 0 {
                    return Err(malformed_regex(flag, dialect, "unmatched `)`"));
                }
                *group_depth -= 1;
                Ok(GnuToken::GroupClose)
            }
            b'|' => Ok(GnuToken::Alternation),
            b'+' if can_repeat_atom => Ok(GnuToken::Quantifier(RepetitionKind::OneOrMore)),
            b'+' => Ok(GnuToken::Literal(b'+')),
            b'?' if can_repeat_atom => Ok(GnuToken::Quantifier(RepetitionKind::ZeroOrOne)),
            b'?' => Ok(GnuToken::Literal(b'?')),
            b'{' if can_repeat_atom => Ok(GnuToken::Quantifier(lex_basic_bound(
                flag, dialect, pattern, index,
            )?)),
            b'{' => Ok(GnuToken::Literal(b'{')),
            b'w' => Ok(GnuToken::WordByteClass { negated: false }),
            b'W' => Ok(GnuToken::WordByteClass { negated: true }),
            b'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
            b'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
            b'<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
            b'>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
            b'`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
            b'\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
            b'\\' | b'.' | b'^' | b'$' | b'*' | b'[' | b']' | b'}' => {
                Ok(GnuToken::Literal(escaped))
            }
            other => Err(unsupported_construct(
                flag,
                dialect,
                format!("unsupported escape `{}`", escaped_display(other)),
            )),
        },
        RegexDialect::Emacs => match escaped {
            b'(' => {
                *group_depth += 1;
                Ok(GnuToken::GroupOpen)
            }
            b')' => {
                if *group_depth == 0 {
                    return Err(malformed_regex(flag, dialect, "unmatched `)`"));
                }
                *group_depth -= 1;
                Ok(GnuToken::GroupClose)
            }
            b'|' => Ok(GnuToken::Alternation),
            b'{' if can_repeat_atom => Ok(GnuToken::Quantifier(lex_basic_bound(
                flag, dialect, pattern, index,
            )?)),
            b'1'..=b'9' => Ok(GnuToken::Backreference((escaped - b'0') as u16)),
            b'w' => Ok(GnuToken::WordByteClass { negated: false }),
            b'W' => Ok(GnuToken::WordByteClass { negated: true }),
            b'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
            b'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
            b'<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
            b'>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
            b'`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
            b'\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
            b'\\' | b'.' | b'^' | b'$' | b'*' | b'+' | b'?' | b'[' | b']' | b'}' => {
                Ok(GnuToken::Literal(escaped))
            }
            other => Ok(GnuToken::Literal(other)),
        },
        RegexDialect::PosixExtended | RegexDialect::Rust | RegexDialect::Pcre2 => unreachable!(),
    }
}

fn lex_extended_escape(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<GnuToken, Diagnostic> {
    let escaped = pattern
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "trailing `\\`"))?;
    *index += 1;

    match escaped {
        b'1'..=b'9' => Ok(GnuToken::Backreference((escaped - b'0') as u16)),
        b'w' => Ok(GnuToken::WordByteClass { negated: false }),
        b'W' => Ok(GnuToken::WordByteClass { negated: true }),
        b'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
        b'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
        b'<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
        b'>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
        b'`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
        b'\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
        b'\\' | b'.' | b'^' | b'$' | b'*' | b'+' | b'?' | b'(' | b')' | b'|' | b'{' | b'}'
        | b'[' | b']' => Ok(GnuToken::Literal(escaped)),
        other => Err(unsupported_construct(
            flag,
            dialect,
            format!("unsupported escape `{}`", escaped_display(other)),
        )),
    }
}

fn lex_bre_or_emacs_escape_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
    can_repeat_atom: bool,
    group_depth: &mut usize,
) -> Result<GnuToken, Diagnostic> {
    let escaped = units
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "trailing `\\`"))?
        .as_char()
        .ok_or_else(|| malformed_regex(flag, dialect, "invalid encoded character"))?;
    *index += 1;

    match dialect {
        RegexDialect::PosixBasic => match escaped {
            '1'..='9' => Ok(GnuToken::Backreference((escaped as u8 - b'0') as u16)),
            '(' => {
                *group_depth += 1;
                Ok(GnuToken::GroupOpen)
            }
            ')' => {
                if *group_depth == 0 {
                    return Err(malformed_regex(flag, dialect, "unmatched `)`"));
                }
                *group_depth -= 1;
                Ok(GnuToken::GroupClose)
            }
            '|' => Ok(GnuToken::Alternation),
            '+' if can_repeat_atom => Ok(GnuToken::Quantifier(RepetitionKind::OneOrMore)),
            '+' => Ok(GnuToken::Literal(b'+')),
            '?' if can_repeat_atom => Ok(GnuToken::Quantifier(RepetitionKind::ZeroOrOne)),
            '?' => Ok(GnuToken::Literal(b'?')),
            '{' if can_repeat_atom => Ok(GnuToken::Quantifier(lex_basic_bound_units(
                flag, dialect, units, index,
            )?)),
            '{' => Ok(GnuToken::Literal(b'{')),
            'w' => Ok(GnuToken::WordByteClass { negated: false }),
            'W' => Ok(GnuToken::WordByteClass { negated: true }),
            'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
            'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
            '<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
            '>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
            '`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
            '\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
            '\\' | '.' | '^' | '$' | '*' | '[' | ']' | '}' => Ok(GnuToken::Literal(escaped as u8)),
            other => Err(unsupported_construct(
                flag,
                dialect,
                format!("unsupported escape `{}`", escaped_display_char(other)),
            )),
        },
        RegexDialect::Emacs => match escaped {
            '(' => {
                *group_depth += 1;
                Ok(GnuToken::GroupOpen)
            }
            ')' => {
                if *group_depth == 0 {
                    return Err(malformed_regex(flag, dialect, "unmatched `)`"));
                }
                *group_depth -= 1;
                Ok(GnuToken::GroupClose)
            }
            '|' => Ok(GnuToken::Alternation),
            '{' if can_repeat_atom => Ok(GnuToken::Quantifier(lex_basic_bound_units(
                flag, dialect, units, index,
            )?)),
            '1'..='9' => Ok(GnuToken::Backreference((escaped as u8 - b'0') as u16)),
            'w' => Ok(GnuToken::WordByteClass { negated: false }),
            'W' => Ok(GnuToken::WordByteClass { negated: true }),
            'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
            'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
            '<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
            '>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
            '`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
            '\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
            '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '[' | ']' | '}' => {
                Ok(GnuToken::Literal(escaped as u8))
            }
            other => Ok(literal_token_for_char(other)),
        },
        RegexDialect::PosixExtended | RegexDialect::Rust | RegexDialect::Pcre2 => unreachable!(),
    }
}

fn lex_extended_escape_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<GnuToken, Diagnostic> {
    let escaped = units
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "trailing `\\`"))?
        .as_char()
        .ok_or_else(|| malformed_regex(flag, dialect, "invalid encoded character"))?;
    *index += 1;

    match escaped {
        '1'..='9' => Ok(GnuToken::Backreference((escaped as u8 - b'0') as u16)),
        'w' => Ok(GnuToken::WordByteClass { negated: false }),
        'W' => Ok(GnuToken::WordByteClass { negated: true }),
        'b' => Ok(GnuToken::Assertion(AssertionKind::WordBoundary)),
        'B' => Ok(GnuToken::Assertion(AssertionKind::NotWordBoundary)),
        '<' => Ok(GnuToken::Assertion(AssertionKind::WordStart)),
        '>' => Ok(GnuToken::Assertion(AssertionKind::WordEnd)),
        '`' => Ok(GnuToken::Assertion(AssertionKind::BufferStart)),
        '\'' => Ok(GnuToken::Assertion(AssertionKind::BufferEnd)),
        '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '|' | '{' | '}' | '[' | ']' => {
            Ok(GnuToken::Literal(escaped as u8))
        }
        other => Err(unsupported_construct(
            flag,
            dialect,
            format!("unsupported escape `{}`", escaped_display_char(other)),
        )),
    }
}

fn emacs_dollar_is_anchor(pattern: &[u8], index: usize) -> bool {
    match pattern.get(index).copied() {
        None => true,
        Some(b'\\') => matches!(pattern.get(index + 1).copied(), Some(b')') | Some(b'|')),
        Some(_) => false,
    }
}

fn emacs_dollar_is_anchor_units(units: &[TextUnit<'_>], index: usize) -> bool {
    match units.get(index).and_then(|unit| unit.as_char()) {
        None => true,
        Some('\\') => matches!(
            units.get(index + 1).and_then(|unit| unit.as_char()),
            Some(')') | Some('|')
        ),
        Some(_) => false,
    }
}

fn lex_extended_bound(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<RepetitionKind, Diagnostic> {
    let start = *index;
    while let Some(byte) = pattern.get(*index).copied() {
        *index += 1;
        if byte == b'}' {
            let body = std::str::from_utf8(&pattern[start..*index - 1])
                .map_err(|_| malformed_regex(flag, dialect, "malformed bounded repetition"))?;
            return parse_repetition_body(flag, dialect, body);
        }
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bounded repetition",
    ))
}

fn lex_basic_bound(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<RepetitionKind, Diagnostic> {
    let start = *index;
    while *index + 1 < pattern.len() {
        if pattern[*index] == b'\\' && pattern[*index + 1] == b'}' {
            let body = std::str::from_utf8(&pattern[start..*index])
                .map_err(|_| malformed_regex(flag, dialect, "malformed bounded repetition"))?;
            *index += 2;
            return parse_repetition_body(flag, dialect, body);
        }
        *index += 1;
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bounded repetition",
    ))
}

fn lex_extended_bound_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<RepetitionKind, Diagnostic> {
    let start = *index;
    while let Some(ch) = units.get(*index).and_then(|unit| unit.as_char()) {
        *index += 1;
        if ch == '}' {
            let body = collect_ascii_units(flag, dialect, &units[start..*index - 1])?;
            return parse_repetition_body(flag, dialect, &body);
        }
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bounded repetition",
    ))
}

fn lex_basic_bound_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<RepetitionKind, Diagnostic> {
    let start = *index;
    while *index + 1 < units.len() {
        if units[*index].as_char() == Some('\\') && units[*index + 1].as_char() == Some('}') {
            let body = collect_ascii_units(flag, dialect, &units[start..*index])?;
            *index += 2;
            return parse_repetition_body(flag, dialect, &body);
        }
        *index += 1;
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bounded repetition",
    ))
}

fn collect_ascii_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
) -> Result<String, Diagnostic> {
    let mut body = String::new();
    for unit in units {
        let ch = unit
            .as_char()
            .ok_or_else(|| malformed_regex(flag, dialect, "malformed bounded repetition"))?;
        if !ch.is_ascii() {
            return Err(malformed_regex(
                flag,
                dialect,
                "malformed bounded repetition",
            ));
        }
        body.push(ch);
    }
    Ok(body)
}

fn parse_repetition_body(
    flag: &str,
    dialect: RegexDialect,
    body: &str,
) -> Result<RepetitionKind, Diagnostic> {
    let invalid = || malformed_regex(flag, dialect, "malformed bounded repetition");

    if let Some((left, right)) = body.split_once(',') {
        let left_valid = left.is_empty() || left.chars().all(|ch| ch.is_ascii_digit());
        let right_valid = right.is_empty() || right.chars().all(|ch| ch.is_ascii_digit());
        if !left_valid || !right_valid {
            return Err(invalid());
        }

        let min = if left.is_empty() {
            0
        } else {
            left.parse::<u32>().map_err(|_| invalid())?
        };
        let max = if right.is_empty() {
            None
        } else {
            Some(right.parse::<u32>().map_err(|_| invalid())?)
        };

        Ok(RepetitionKind::Bounded { min, max })
    } else if !body.is_empty() && body.chars().all(|ch| ch.is_ascii_digit()) {
        let exact = body.parse::<u32>().map_err(|_| invalid())?;
        Ok(RepetitionKind::Bounded {
            min: exact,
            max: Some(exact),
        })
    } else {
        Err(invalid())
    }
}

fn lex_class(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<ClassExpr, Diagnostic> {
    let mut negated = false;
    let mut items = Vec::new();

    if pattern.get(*index) == Some(&b'^') {
        *index += 1;
        negated = true;
    }

    if pattern.get(*index) == Some(&b']') {
        *index += 1;
        items.push(ClassItem::Byte(b']'));
    }

    while let Some(byte) = pattern.get(*index).copied() {
        if byte == b']' {
            *index += 1;
            return Ok(ClassExpr { negated, items });
        }

        let item = lex_class_item(flag, dialect, pattern, index)?;
        if matches!(item, ClassItem::Byte(b'-'))
            && !items.is_empty()
            && pattern.get(*index) != Some(&b']')
        {
            let start = items.pop().unwrap();
            let end = lex_class_item(flag, dialect, pattern, index)?;
            match (start, end) {
                (ClassItem::Byte(start), ClassItem::Byte(end)) => {
                    if start > end {
                        return Err(malformed_regex(
                            flag,
                            dialect,
                            "invalid range in bracket expression",
                        ));
                    }
                    items.push(ClassItem::Range(start, end));
                }
                (start, end) => {
                    items.push(start);
                    items.push(ClassItem::Byte(b'-'));
                    items.push(end);
                }
            }
        } else {
            items.push(item);
        }
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bracket expression",
    ))
}

fn lex_class_item(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<ClassItem, Diagnostic> {
    let byte = pattern
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "unterminated bracket expression"))?;
    *index += 1;

    match byte {
        b'[' => match pattern.get(*index).copied() {
            Some(b':') => {
                *index += 1;
                let name = take_posix_class_name(flag, dialect, pattern, index)?;
                Ok(ClassItem::PosixClass(name))
            }
            Some(b'.') => Err(unsupported_construct(
                flag,
                dialect,
                "POSIX collating symbols are out of scope",
            )),
            Some(b'=') => Err(unsupported_construct(
                flag,
                dialect,
                "POSIX equivalence classes are out of scope",
            )),
            _ => Ok(ClassItem::Byte(b'[')),
        },
        b'\\' => Ok(ClassItem::Byte(b'\\')),
        other => Ok(ClassItem::Byte(other)),
    }
}

fn lex_class_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<ClassExpr, Diagnostic> {
    let mut negated = false;
    let mut items = Vec::new();

    if units.get(*index).and_then(|unit| unit.as_char()) == Some('^') {
        *index += 1;
        negated = true;
    }

    if units.get(*index).and_then(|unit| unit.as_char()) == Some(']') {
        *index += 1;
        items.push(ClassItem::Byte(b']'));
    }

    while let Some(unit) = units.get(*index).copied() {
        if unit.as_char() == Some(']') {
            *index += 1;
            return Ok(ClassExpr { negated, items });
        }

        let item = lex_class_item_units(flag, dialect, units, index)?;
        if matches!(item, ClassItem::Byte(b'-'))
            && !items.is_empty()
            && units.get(*index).and_then(|unit| unit.as_char()) != Some(']')
        {
            let start = items.pop().unwrap();
            let end = lex_class_item_units(flag, dialect, units, index)?;
            match (start, end) {
                (ClassItem::Byte(start), ClassItem::Byte(end)) => {
                    if start > end {
                        return Err(malformed_regex(
                            flag,
                            dialect,
                            "invalid range in bracket expression",
                        ));
                    }
                    items.push(ClassItem::Range(start, end));
                }
                (ClassItem::Char(start), ClassItem::Char(end)) => {
                    if start > end {
                        return Err(malformed_regex(
                            flag,
                            dialect,
                            "invalid range in bracket expression",
                        ));
                    }
                    items.push(ClassItem::CharRange(start, end));
                }
                (start, end) => {
                    items.push(start);
                    items.push(ClassItem::Byte(b'-'));
                    items.push(end);
                }
            }
        } else {
            items.push(item);
        }
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated bracket expression",
    ))
}

fn lex_class_item_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<ClassItem, Diagnostic> {
    let ch = units
        .get(*index)
        .copied()
        .ok_or_else(|| malformed_regex(flag, dialect, "unterminated bracket expression"))?
        .as_char()
        .ok_or_else(|| malformed_regex(flag, dialect, "invalid encoded character"))?;
    *index += 1;

    match ch {
        '[' => match units.get(*index).and_then(|unit| unit.as_char()) {
            Some(':') => {
                *index += 1;
                let name = take_posix_class_name_units(flag, dialect, units, index)?;
                Ok(ClassItem::PosixClass(name))
            }
            Some('.') => Err(unsupported_construct(
                flag,
                dialect,
                "POSIX collating symbols are out of scope",
            )),
            Some('=') => Err(unsupported_construct(
                flag,
                dialect,
                "POSIX equivalence classes are out of scope",
            )),
            _ => Ok(ClassItem::Byte(b'[')),
        },
        '\\' => Ok(ClassItem::Byte(b'\\')),
        other if other.is_ascii() => Ok(ClassItem::Byte(other as u8)),
        other => Ok(ClassItem::Char(other)),
    }
}

fn take_posix_class_name(
    flag: &str,
    dialect: RegexDialect,
    pattern: &[u8],
    index: &mut usize,
) -> Result<&'static str, Diagnostic> {
    let start = *index;

    while *index + 1 < pattern.len() {
        if pattern[*index] == b':' && pattern[*index + 1] == b']' {
            let name = std::str::from_utf8(&pattern[start..*index]).map_err(|_| {
                unsupported_construct(flag, dialect, "unsupported POSIX character class")
            })?;
            *index += 2;
            return canonical_posix_class(flag, dialect, name);
        }
        if !pattern[*index].is_ascii() {
            return Err(unsupported_construct(
                flag,
                dialect,
                "unsupported POSIX character class",
            ));
        }
        *index += 1;
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated POSIX character class",
    ))
}

fn take_posix_class_name_units(
    flag: &str,
    dialect: RegexDialect,
    units: &[TextUnit<'_>],
    index: &mut usize,
) -> Result<&'static str, Diagnostic> {
    let mut name = String::new();

    while *index + 1 < units.len() {
        if units[*index].as_char() == Some(':') && units[*index + 1].as_char() == Some(']') {
            *index += 2;
            return canonical_posix_class(flag, dialect, &name);
        }

        let ch = units[*index].as_char().ok_or_else(|| {
            unsupported_construct(flag, dialect, "unsupported POSIX character class")
        })?;
        if !ch.is_ascii() {
            return Err(unsupported_construct(
                flag,
                dialect,
                "unsupported POSIX character class",
            ));
        }
        name.push(ch);
        *index += 1;
    }

    Err(malformed_regex(
        flag,
        dialect,
        "unterminated POSIX character class",
    ))
}

fn canonical_posix_class(
    flag: &str,
    dialect: RegexDialect,
    name: &str,
) -> Result<&'static str, Diagnostic> {
    match name {
        "alnum" => Ok("alnum"),
        "alpha" => Ok("alpha"),
        "blank" => Ok("blank"),
        "cntrl" => Ok("cntrl"),
        "digit" => Ok("digit"),
        "graph" => Ok("graph"),
        "lower" => Ok("lower"),
        "print" => Ok("print"),
        "punct" => Ok("punct"),
        "space" => Ok("space"),
        "upper" => Ok("upper"),
        "xdigit" => Ok("xdigit"),
        other => Err(unsupported_construct(
            flag,
            dialect,
            format!("unsupported POSIX character class `[:{other}:]`"),
        )),
    }
}

struct TokenParser<'a> {
    flag: &'a str,
    dialect: RegexDialect,
    tokens: &'a [GnuToken],
    index: usize,
    next_capture: u16,
}

impl<'a> TokenParser<'a> {
    fn new(flag: &'a str, dialect: RegexDialect, tokens: &'a [GnuToken]) -> Self {
        Self {
            flag,
            dialect,
            tokens,
            index: 0,
            next_capture: 0,
        }
    }

    fn parse(mut self) -> Result<GnuRegex, Diagnostic> {
        let expr = self.parse_alternation()?;
        if self.index != self.tokens.len() {
            return Err(malformed_regex(self.flag, self.dialect, "trailing input"));
        }
        Ok(GnuRegex {
            expr,
            capture_count: self.next_capture,
        })
    }

    fn parse_alternation(&mut self) -> Result<GnuExpr, Diagnostic> {
        let mut branches = vec![self.parse_sequence()?];
        while matches!(self.peek(), Some(GnuToken::Alternation)) {
            self.index += 1;
            branches.push(self.parse_sequence()?);
        }

        Ok(match branches.len() {
            1 => branches.pop().unwrap(),
            _ => GnuExpr::Alternation(branches),
        })
    }

    fn parse_sequence(&mut self) -> Result<GnuExpr, Diagnostic> {
        let mut items = Vec::new();
        while let Some(token) = self.peek().cloned() {
            match token {
                GnuToken::GroupClose | GnuToken::Alternation => break,
                GnuToken::Quantifier(kind) => {
                    self.index += 1;
                    let expr = items.pop().ok_or_else(|| {
                        malformed_regex(
                            self.flag,
                            self.dialect,
                            "repetition operator missing expression",
                        )
                    })?;
                    items.push(GnuExpr::Repeat {
                        expr: Box::new(expr),
                        kind,
                    });
                }
                _ => items.push(self.parse_atom()?),
            }
        }

        Ok(match items.len() {
            0 => GnuExpr::Empty,
            1 => items.pop().unwrap(),
            _ => GnuExpr::Concat(items),
        })
    }

    fn parse_atom(&mut self) -> Result<GnuExpr, Diagnostic> {
        match self.next().unwrap() {
            GnuToken::Literal(byte) => Ok(GnuExpr::Literal(byte)),
            GnuToken::LiteralChar(ch) => Ok(GnuExpr::LiteralChar(ch)),
            GnuToken::Dot => Ok(GnuExpr::Dot),
            GnuToken::AnchorStart => Ok(GnuExpr::Anchor(AnchorKind::Start)),
            GnuToken::AnchorEnd => Ok(GnuExpr::Anchor(AnchorKind::End)),
            GnuToken::Class(expr) => Ok(GnuExpr::Class(expr)),
            GnuToken::GroupOpen => self.parse_group(),
            GnuToken::GroupClose => Err(malformed_regex(self.flag, self.dialect, "unmatched `)`")),
            GnuToken::Alternation => Ok(GnuExpr::Empty),
            GnuToken::Backreference(index) => Ok(GnuExpr::Backreference(index)),
            GnuToken::Assertion(kind) => Ok(GnuExpr::Assertion(kind)),
            GnuToken::WordByteClass { negated } => Ok(GnuExpr::WordByteClass { negated }),
            GnuToken::Quantifier(_) => Err(malformed_regex(
                self.flag,
                self.dialect,
                "repetition operator missing expression",
            )),
        }
    }

    fn parse_group(&mut self) -> Result<GnuExpr, Diagnostic> {
        self.next_capture += 1;
        let capture_index = self.next_capture;
        let expr = self.parse_alternation()?;
        if !matches!(self.next(), Some(GnuToken::GroupClose)) {
            return Err(malformed_regex(self.flag, self.dialect, "unclosed group"));
        }
        Ok(GnuExpr::Group {
            capture_index,
            expr: Box::new(expr),
        })
    }

    fn next(&mut self) -> Option<GnuToken> {
        let token = self.tokens.get(self.index).cloned()?;
        self.index += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&GnuToken> {
        self.tokens.get(self.index)
    }
}

fn lower_to_rust(expr: &GnuExpr) -> Result<String, Diagnostic> {
    let mut out = String::new();
    render_expr(expr, &mut out, RegexBackendKind::Rust)?;
    Ok(out)
}

fn lower_to_pcre2(expr: &GnuExpr) -> Result<String, Diagnostic> {
    let mut out = String::new();
    render_expr(expr, &mut out, RegexBackendKind::Pcre2)?;
    Ok(out)
}

fn render_expr(
    expr: &GnuExpr,
    out: &mut String,
    backend: RegexBackendKind,
) -> Result<(), Diagnostic> {
    match expr {
        GnuExpr::Empty => {}
        GnuExpr::Literal(byte) => push_literal_regex_byte(out, *byte),
        GnuExpr::LiteralChar(ch) => push_literal_regex_char(out, *ch),
        GnuExpr::Dot => out.push('.'),
        GnuExpr::Concat(items) => {
            for item in items {
                render_expr(item, out, backend)?;
            }
        }
        GnuExpr::Alternation(items) => {
            out.push_str("(?:");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push('|');
                }
                render_expr(item, out, backend)?;
            }
            out.push(')');
        }
        GnuExpr::Group { expr, .. } => {
            out.push('(');
            render_expr(expr, out, backend)?;
            out.push(')');
        }
        GnuExpr::Class(class) => {
            out.push('[');
            if class.negated {
                out.push('^');
            }
            for item in &class.items {
                match item {
                    ClassItem::Byte(byte) => push_bracket_escaped_byte(out, *byte),
                    ClassItem::Char(ch) => push_bracket_escaped_char(out, *ch),
                    ClassItem::Range(start, end) => {
                        push_bracket_range_endpoint_byte(out, *start);
                        out.push('-');
                        push_bracket_range_endpoint_byte(out, *end);
                    }
                    ClassItem::CharRange(start, end) => {
                        push_bracket_range_endpoint_char(out, *start);
                        out.push('-');
                        push_bracket_range_endpoint_char(out, *end);
                    }
                    ClassItem::PosixClass(name) => out.push_str(posix_named_class_fragment(name)),
                }
            }
            out.push(']');
        }
        GnuExpr::Repeat { expr, kind } => {
            render_repeated_expr(expr, out, backend)?;
            match kind {
                RepetitionKind::ZeroOrMore => out.push('*'),
                RepetitionKind::OneOrMore => out.push('+'),
                RepetitionKind::ZeroOrOne => out.push('?'),
                RepetitionKind::Bounded { min, max } => match max {
                    Some(max) if *max == *min => write!(out, "{{{min}}}").unwrap(),
                    Some(max) => write!(out, "{{{min},{max}}}").unwrap(),
                    None => write!(out, "{{{min},}}").unwrap(),
                },
            }
        }
        GnuExpr::Anchor(AnchorKind::Start) => out.push('^'),
        GnuExpr::Anchor(AnchorKind::End) => out.push('$'),
        GnuExpr::Backreference(index) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            write!(out, r"\{}", index).unwrap();
        }
        GnuExpr::Assertion(AssertionKind::WordBoundary) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\b");
        }
        GnuExpr::Assertion(AssertionKind::NotWordBoundary) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\B");
        }
        GnuExpr::Assertion(AssertionKind::WordStart) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\b(?=\w)");
        }
        GnuExpr::Assertion(AssertionKind::WordEnd) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\b(?<=\w)");
        }
        GnuExpr::Assertion(AssertionKind::BufferStart) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\A");
        }
        GnuExpr::Assertion(AssertionKind::BufferEnd) => {
            debug_assert_eq!(backend, RegexBackendKind::Pcre2);
            out.push_str(r"\z");
        }
        GnuExpr::WordByteClass { negated: false } => out.push_str(r"\w"),
        GnuExpr::WordByteClass { negated: true } => out.push_str(r"\W"),
    }
    Ok(())
}

fn render_repeated_expr(
    expr: &GnuExpr,
    out: &mut String,
    backend: RegexBackendKind,
) -> Result<(), Diagnostic> {
    match expr {
        GnuExpr::Alternation(_) | GnuExpr::Concat(_) => {
            out.push_str("(?:");
            render_expr(expr, out, backend)?;
            out.push(')');
            Ok(())
        }
        _ => render_expr(expr, out, backend),
    }
}

fn push_literal_regex_byte(out: &mut String, byte: u8) {
    match byte {
        b'.' | b'^' | b'$' | b'|' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'*' | b'+'
        | b'?' | b'\\' => {
            out.push('\\');
            out.push(char::from(byte));
        }
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_literal_regex_char(out: &mut String, ch: char) {
    match ch {
        '.' | '^' | '$' | '|' | '(' | ')' | '[' | ']' | '{' | '}' | '*' | '+' | '?' | '\\' => {
            out.push('\\');
            out.push(ch);
        }
        other => out.push(other),
    }
}

fn push_bracket_escaped_byte(out: &mut String, byte: u8) {
    match byte {
        b'\\' | b']' | b'^' | b'-' => {
            out.push('\\');
            out.push(char::from(byte));
        }
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_bracket_escaped_char(out: &mut String, ch: char) {
    match ch {
        '\\' | ']' | '^' | '-' => {
            out.push('\\');
            out.push(ch);
        }
        other => out.push(other),
    }
}

fn push_bracket_range_endpoint_byte(out: &mut String, byte: u8) {
    match byte {
        b'\\' | b']' | b'^' => {
            out.push('\\');
            out.push(char::from(byte));
        }
        0x20..=0x7e => out.push(char::from(byte)),
        other => push_hex_byte(out, other),
    }
}

fn push_bracket_range_endpoint_char(out: &mut String, ch: char) {
    match ch {
        '\\' | ']' | '^' => {
            out.push('\\');
            out.push(ch);
        }
        other => out.push(other),
    }
}

fn push_hex_byte(out: &mut String, byte: u8) {
    write!(out, r"\x{:02X}", byte).unwrap();
}

fn escaped_display(byte: u8) -> String {
    match byte {
        0x20..=0x7e => format!(r"\{}", char::from(byte)),
        _ => format!(r"\x{:02X}", byte),
    }
}

fn escaped_display_char(ch: char) -> String {
    if ch.is_ascii() {
        escaped_display(ch as u8)
    } else {
        format!(r"\{ch}")
    }
}

fn posix_named_class_fragment(name: &str) -> &'static str {
    match name {
        "alnum" => "A-Za-z0-9",
        "alpha" => "A-Za-z",
        "blank" => r" \t",
        "cntrl" => r"\x00-\x1F\x7F",
        "digit" => "0-9",
        "graph" => "!-~",
        "lower" => "a-z",
        "print" => r"\x20-\x7E",
        "punct" => r"!-/:-@\x5B-\x60{-~",
        "space" => r" \t\r\n\f\x0B",
        "upper" => "A-Z",
        "xdigit" => "A-Fa-f0-9",
        _ => unreachable!("POSIX classes are validated during lexing"),
    }
}

fn unsupported_construct(
    flag: &str,
    dialect: RegexDialect,
    reason: impl std::fmt::Display,
) -> Diagnostic {
    Diagnostic::new(
        format!(
            "unsupported construct in {} regex for `{flag}`: {reason}",
            dialect.label()
        ),
        1,
    )
}

fn malformed_regex(
    flag: &str,
    dialect: RegexDialect,
    reason: impl std::fmt::Display,
) -> Diagnostic {
    Diagnostic::new(
        format!("malformed {} regex for `{flag}`: {reason}", dialect.label()),
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::{choose_backend, parse_gnu_regex};
    use crate::regex_match::RegexDialect;
    use crate::regex_match::backend::RegexBackendKind;

    #[test]
    fn gnu_ir_subset_tracks_capturing_groups_for_basic_regexes() {
        let expr = parse_gnu_regex(
            "-regex",
            RegexDialect::PosixBasic,
            br".*/src/\(lib\|main\)\.rs",
        )
        .unwrap();

        assert_eq!(expr.capture_count(), 1);
        assert_eq!(choose_backend(&expr), RegexBackendKind::Rust);
    }

    #[test]
    fn gnu_ir_subset_keeps_posix_named_classes_in_the_rust_fast_path() {
        let expr = parse_gnu_regex(
            "-regex",
            RegexDialect::PosixExtended,
            br".*/[[:upper:]][[:alpha:]]*\.MD",
        )
        .unwrap();

        assert_eq!(choose_backend(&expr), RegexBackendKind::Rust);
    }

    #[test]
    fn gnu_foundation_ere_unmatched_close_paren_is_literal() {
        let expr = parse_gnu_regex("-regex", RegexDialect::PosixExtended, br".*/paren)").unwrap();

        assert_eq!(choose_backend(&expr), RegexBackendKind::Rust);
    }

    #[test]
    fn gnu_foundation_backreferences_force_pcre2_fallback() {
        let expr = parse_gnu_regex("-regex", RegexDialect::PosixBasic, br".*/\(.\)\1").unwrap();

        assert_eq!(choose_backend(&expr), RegexBackendKind::Pcre2);
    }

    #[test]
    fn emacs_followup_backreferences_force_pcre2_fallback() {
        let expr = parse_gnu_regex("-regex", RegexDialect::Emacs, br".*/\(.\)\1").unwrap();

        assert_eq!(expr.capture_count(), 1);
        assert_eq!(choose_backend(&expr), RegexBackendKind::Pcre2);
    }

    #[test]
    fn emacs_followup_mixed_alternation_and_backreference_force_pcre2_fallback() {
        let expr = parse_gnu_regex("-regex", RegexDialect::Emacs, br".*/\(ab\|cd\)\1").unwrap();

        assert_eq!(expr.capture_count(), 1);
        assert_eq!(choose_backend(&expr), RegexBackendKind::Pcre2);
    }

    #[test]
    fn gnu_hardening_focus_invalid_backreference_is_rejected_before_backend_compile() {
        let error = parse_gnu_regex("-regex", RegexDialect::PosixExtended, br".*/\1").unwrap_err();

        assert!(error.message.contains("invalid back reference"));
    }

    #[test]
    fn gnu_hardening_focus_invalid_bounded_repetition_is_rejected_before_backend_compile() {
        let error =
            parse_gnu_regex("-regex", RegexDialect::PosixBasic, br".*/a\{2,1\}").unwrap_err();

        assert!(error.message.contains("invalid bounded repetition"));
    }

    #[test]
    fn gnu_foundation_boundary_escapes_force_pcre2_fallback() {
        let expr = parse_gnu_regex("-regex", RegexDialect::PosixExtended, br".*/\<foo\>").unwrap();

        assert_eq!(choose_backend(&expr), RegexBackendKind::Pcre2);
    }
}
