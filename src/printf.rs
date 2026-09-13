use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::eval::EvalContext;
use crate::follow::FollowMode;
use crate::platform;
use crate::printf_time::{ResolvedTimeParts, resolve_local_time_parts};
use std::ffi::OsStr;

mod value;
use value::render_directive_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfProgram {
    pub atoms: Vec<PrintfAtom>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPrintfProgram {
    pub program: PrintfProgram,
    pub warnings: Vec<String>,
}

impl PrintfProgram {
    pub fn requires_mount_snapshot(&self) -> bool {
        self.atoms.iter().any(|atom| {
            matches!(
                atom,
                PrintfAtom::Directive(PrintfDirective {
                    kind: PrintfDirectiveKind::FileSystemType,
                    ..
                })
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrintfFieldFormat {
    pub left_align: bool,
    pub zero_pad: bool,
    pub always_sign: bool,
    pub space_sign: bool,
    pub alternate: bool,
    pub width: Option<usize>,
    pub precision: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintfTimeFamily {
    Access,
    Change,
    Modification,
    Birth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintfTimeSelector {
    Byte(u8),
    EpochSeconds,
    GnuPlus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintfDirectiveKind {
    Path,
    RelativePath,
    StartPath,
    Basename,
    Dirname,
    Depth,
    FileType,
    FileTypeFollow,
    Size,
    Sparseness,
    ModeOctal,
    ModeSymbolic,
    LinkTarget,
    Inode,
    LinkCount,
    Device,
    Blocks512,
    Blocks1024,
    UserName,
    UserId,
    UserSid,
    GroupName,
    GroupId,
    GroupSid,
    FileSystemType,
    FullTimestamp(PrintfTimeFamily),
    TimestampPart {
        family: PrintfTimeFamily,
        selector: PrintfTimeSelector,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintfDirective {
    pub kind: PrintfDirectiveKind,
    pub format: PrintfFieldFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintfAtom {
    Literal(Vec<u8>),
    Directive(PrintfDirective),
    Stop,
}

pub fn compile_printf_program(
    flag: &str,
    format: &OsStr,
) -> Result<CompiledPrintfProgram, Diagnostic> {
    let bytes = format.as_encoded_bytes();
    let mut atoms = Vec::new();
    let mut literal = Vec::new();
    let mut warnings = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if !literal.is_empty() {
                    atoms.push(PrintfAtom::Literal(std::mem::take(&mut literal)));
                }

                let start = index;
                index += 1;
                let directive = *bytes.get(index).ok_or_else(|| {
                    Diagnostic::new(format!("malformed {flag} format: trailing %"), 1)
                })?;

                if directive == b'%' {
                    atoms.push(PrintfAtom::Literal(vec![b'%']));
                } else {
                    match parse_directive(flag, bytes, &mut index)? {
                        ParsedDirective::Known(parsed) => {
                            atoms.push(PrintfAtom::Directive(parsed));
                        }
                        ParsedDirective::TruncatedTimeFamily(letter) => {
                            warnings.push(format!(
                                "rfd: warning: format directive `%{}' should be followed by another character",
                                char::from(letter)
                            ));
                            atoms.push(PrintfAtom::Literal(bytes[start..=index].to_vec()));
                        }
                        ParsedDirective::Unrecognized(letter) => {
                            warnings.push(format!(
                                "rfd: warning: unrecognized format directive `%{}'",
                                char::from(letter)
                            ));
                            atoms.push(PrintfAtom::Literal(bytes[start..=index].to_vec()));
                        }
                    }
                }
            }
            b'\\' => {
                index += 1;
                parse_escape(
                    flag,
                    bytes,
                    &mut index,
                    &mut atoms,
                    &mut literal,
                    &mut warnings,
                )?;
            }
            byte => literal.push(byte),
        }

        index += 1;
    }

    if !literal.is_empty() {
        atoms.push(PrintfAtom::Literal(literal));
    }

    Ok(CompiledPrintfProgram {
        program: PrintfProgram { atoms },
        warnings,
    })
}

fn parse_escape(
    flag: &str,
    bytes: &[u8],
    index: &mut usize,
    atoms: &mut Vec<PrintfAtom>,
    literal: &mut Vec<u8>,
    warnings: &mut Vec<String>,
) -> Result<(), Diagnostic> {
    let escaped = *bytes
        .get(*index)
        .ok_or_else(|| Diagnostic::new(format!("malformed {flag} format: trailing \\"), 1))?;

    match escaped {
        b'a' => literal.push(0x07),
        b'b' => literal.push(0x08),
        b'c' => {
            if !literal.is_empty() {
                atoms.push(PrintfAtom::Literal(std::mem::take(literal)));
            }
            atoms.push(PrintfAtom::Stop);
        }
        b'f' => literal.push(0x0c),
        b'n' => literal.push(b'\n'),
        b'r' => literal.push(b'\r'),
        b't' => literal.push(b'\t'),
        b'v' => literal.push(0x0b),
        b'\\' => literal.push(b'\\'),
        b'0'..=b'7' => literal.push(parse_octal_escape(bytes, index, escaped)),
        other => {
            warnings.push(format!(
                "rfd: warning: unrecognized escape `\\{}'",
                char::from(other)
            ));
            literal.extend_from_slice(&[b'\\', other]);
        }
    }

    Ok(())
}

fn parse_octal_escape(bytes: &[u8], index: &mut usize, first: u8) -> u8 {
    let mut value = u16::from(first - b'0');
    for _ in 0..2 {
        let Some(next) = bytes.get(*index + 1).copied() else {
            break;
        };
        if !(b'0'..=b'7').contains(&next) {
            break;
        }
        *index += 1;
        value = (value * 8) + u16::from(next - b'0');
    }
    value as u8
}

/// A single `%`-introduced specifier. GNU find emits unrecognized directives
/// verbatim, so the parser hands the letter back instead of failing.
enum ParsedDirective {
    Known(PrintfDirective),
    Unrecognized(u8),
    /// A time family letter that the format ends on. GNU warns that it should
    /// be followed by another character and emits it as it stands.
    TruncatedTimeFamily(u8),
}

fn parse_directive(
    flag: &str,
    bytes: &[u8],
    index: &mut usize,
) -> Result<ParsedDirective, Diagnostic> {
    let mut format = PrintfFieldFormat::default();

    // GNU find only scans `-`, `+`, ` ` and `#` as flags; a leading zero belongs
    // to the width, so `%0-8s` is unrecognized while `%-08s` is not.
    loop {
        match bytes.get(*index).copied() {
            Some(b'-') => format.left_align = true,
            Some(b'+') => format.always_sign = true,
            Some(b' ') => format.space_sign = true,
            Some(b'#') => format.alternate = true,
            _ => break,
        }
        *index += 1;
    }

    if bytes.get(*index).is_some_and(|byte| byte.is_ascii_digit()) {
        format.zero_pad = bytes[*index] == b'0';
        format.width = parse_optional_usize(flag, bytes, index)?;
    }
    if bytes.get(*index) == Some(&b'.') {
        *index += 1;
        format.precision = Some(parse_required_usize(flag, bytes, index)?);
    }

    let directive = *bytes
        .get(*index)
        .ok_or_else(|| Diagnostic::new(format!("malformed {flag} format: trailing %"), 1))?;

    let kind = match directive {
        b'a' => PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Access),
        b'c' => PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Change),
        b't' => PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Modification),
        b'B' => parse_birth_directive(bytes, index)?,
        b'A' | b'C' | b'T' => {
            // A format that ends on the family letter warns and emits it, as
            // GNU find does, instead of failing the whole run.
            let Some(selector_byte) = bytes.get(*index + 1).copied() else {
                return Ok(ParsedDirective::TruncatedTimeFamily(directive));
            };
            *index += 1;
            PrintfDirectiveKind::TimestampPart {
                family: time_family(directive),
                selector: parse_time_selector_byte(selector_byte),
            }
        }
        b'p' => PrintfDirectiveKind::Path,
        b'P' => PrintfDirectiveKind::RelativePath,
        b'H' => PrintfDirectiveKind::StartPath,
        b'f' => PrintfDirectiveKind::Basename,
        b'h' => PrintfDirectiveKind::Dirname,
        b'd' => PrintfDirectiveKind::Depth,
        b'y' => PrintfDirectiveKind::FileType,
        b'Y' => PrintfDirectiveKind::FileTypeFollow,
        b's' => PrintfDirectiveKind::Size,
        b'S' => PrintfDirectiveKind::Sparseness,
        b'm' => PrintfDirectiveKind::ModeOctal,
        b'M' => PrintfDirectiveKind::ModeSymbolic,
        b'l' => PrintfDirectiveKind::LinkTarget,
        b'i' => PrintfDirectiveKind::Inode,
        b'n' => PrintfDirectiveKind::LinkCount,
        b'D' => PrintfDirectiveKind::Device,
        b'b' => PrintfDirectiveKind::Blocks512,
        b'k' => PrintfDirectiveKind::Blocks1024,
        b'u' => PrintfDirectiveKind::UserName,
        b'U' if bytes.get(*index + 1) == Some(&b'S') => {
            *index += 1;
            PrintfDirectiveKind::UserSid
        }
        b'U' => PrintfDirectiveKind::UserId,
        b'g' => PrintfDirectiveKind::GroupName,
        b'G' if bytes.get(*index + 1) == Some(&b'S') => {
            *index += 1;
            PrintfDirectiveKind::GroupSid
        }
        b'G' => PrintfDirectiveKind::GroupId,
        b'F' => PrintfDirectiveKind::FileSystemType,
        other => return Ok(ParsedDirective::Unrecognized(other)),
    };

    Ok(ParsedDirective::Known(PrintfDirective { kind, format }))
}

fn parse_birth_directive(
    bytes: &[u8],
    index: &mut usize,
) -> Result<PrintfDirectiveKind, Diagnostic> {
    match bytes.get(*index + 1).copied() {
        Some(next) if is_time_selector_lead_byte(next) => {
            *index += 1;
            Ok(PrintfDirectiveKind::TimestampPart {
                family: PrintfTimeFamily::Birth,
                selector: parse_time_selector_byte(next),
            })
        }
        _ => Ok(PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Birth)),
    }
}

fn time_family(directive: u8) -> PrintfTimeFamily {
    match directive {
        b'A' => PrintfTimeFamily::Access,
        b'C' => PrintfTimeFamily::Change,
        b'T' => PrintfTimeFamily::Modification,
        _ => unreachable!("caller restricts directive"),
    }
}

fn is_time_selector_lead_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'@' | b'+')
}

/// GNU find accepts every byte as a time selector and hands it to `strftime`,
/// which is what decides the rendering of the ones neither we nor find knows.
fn parse_time_selector_byte(byte: u8) -> PrintfTimeSelector {
    match byte {
        b'@' => PrintfTimeSelector::EpochSeconds,
        b'+' => PrintfTimeSelector::GnuPlus,
        other => PrintfTimeSelector::Byte(other),
    }
}

fn parse_optional_usize(
    flag: &str,
    bytes: &[u8],
    index: &mut usize,
) -> Result<Option<usize>, Diagnostic> {
    let start = *index;
    while bytes.get(*index).is_some_and(|byte| byte.is_ascii_digit()) {
        *index += 1;
    }

    if start == *index {
        return Ok(None);
    }

    std::str::from_utf8(&bytes[start..*index])
        .unwrap()
        .parse::<usize>()
        .map(Some)
        .map_err(|_| Diagnostic::new(format!("malformed {flag} format: invalid field width"), 1))
}

fn parse_required_usize(flag: &str, bytes: &[u8], index: &mut usize) -> Result<usize, Diagnostic> {
    let start = *index;
    while bytes.get(*index).is_some_and(|byte| byte.is_ascii_digit()) {
        *index += 1;
    }

    if start == *index {
        return Err(Diagnostic::new(
            format!("malformed {flag} format: expected digits after `.`"),
            1,
        ));
    }

    std::str::from_utf8(&bytes[start..*index])
        .unwrap()
        .parse::<usize>()
        .map_err(|_| Diagnostic::new(format!("malformed {flag} format: invalid field width"), 1))
}

pub(crate) fn render_printf_bytes(
    program: &PrintfProgram,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    let mut rendered = Vec::new();
    let mut state = PrintfRenderState::default();

    for atom in &program.atoms {
        match atom {
            PrintfAtom::Literal(bytes) => rendered.extend_from_slice(bytes),
            PrintfAtom::Directive(directive) => rendered.extend_from_slice(
                &render_directive_bytes(directive, entry, follow_mode, context, &mut state)?,
            ),
            PrintfAtom::Stop => break,
        }
    }

    Ok(rendered)
}

#[derive(Default)]
struct PrintfRenderState {
    access: Option<Option<ResolvedTimeParts>>,
    change: Option<Option<ResolvedTimeParts>>,
    modification: Option<Option<ResolvedTimeParts>>,
    birth: Option<Option<ResolvedTimeParts>>,
}

fn resolve_cached_time_parts<'a>(
    state: &'a mut PrintfRenderState,
    family: PrintfTimeFamily,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Option<&'a ResolvedTimeParts>, Diagnostic> {
    let slot = match family {
        PrintfTimeFamily::Access => &mut state.access,
        PrintfTimeFamily::Change => &mut state.change,
        PrintfTimeFamily::Modification => &mut state.modification,
        PrintfTimeFamily::Birth => &mut state.birth,
    };

    if slot.is_none() {
        let timestamp = match family {
            PrintfTimeFamily::Access => Some(entry.active_atime(follow_mode)?),
            PrintfTimeFamily::Change => Some(entry.active_ctime(follow_mode)?),
            PrintfTimeFamily::Modification => Some(entry.active_mtime(follow_mode)?),
            PrintfTimeFamily::Birth => entry.active_birth_time(follow_mode)?,
        };
        *slot = Some(match timestamp {
            Some(timestamp) => Some(resolve_local_time_parts(timestamp)?),
            None => None,
        });
    }

    Ok(slot.as_ref().and_then(|value| value.as_ref()))
}

fn pad_field(value: &[u8], width: Option<usize>, left_align: bool, pad: u8) -> Vec<u8> {
    let Some(width) = width else {
        return value.to_vec();
    };

    if value.len() >= width {
        return value.to_vec();
    }

    let padding = vec![pad; width - value.len()];
    if left_align {
        let mut rendered = Vec::with_capacity(width);
        rendered.extend_from_slice(value);
        rendered.extend_from_slice(&padding);
        rendered
    } else {
        let mut rendered = Vec::with_capacity(width);
        rendered.extend_from_slice(&padding);
        rendered.extend_from_slice(value);
        rendered
    }
}

fn format_string_like(value: &[u8], format: PrintfFieldFormat) -> Vec<u8> {
    let value = match format.precision {
        Some(limit) => &value[..value.len().min(limit)],
        None => value,
    };
    // `-` wins over `0`; otherwise the host C library decides whether `0`
    // reaches a string field, and on the BSDs it does even with a precision.
    let pad = if !format.left_align && format.zero_pad && platform::printf_zero_pads_string_fields()
    {
        b'0'
    } else {
        b' '
    };
    pad_field(value, format.width, format.left_align, pad)
}

fn format_depth(depth: usize, format: PrintfFieldFormat) -> Vec<u8> {
    let sign = if format.always_sign {
        Some(b'+')
    } else if format.space_sign {
        Some(b' ')
    } else {
        None
    };
    let mut digits = depth.to_string().into_bytes();
    suppress_zero_digits(&mut digits, format);
    format_numeric_value(digits, sign, format)
}

/// C prints no digits at all for a zero value under a precision of zero; the
/// sign and the `#` prefix an octal field asks for are still emitted.
fn suppress_zero_digits(digits: &mut Vec<u8>, format: PrintfFieldFormat) {
    if format.precision == Some(0) && digits.iter().all(|byte| *byte == b'0') {
        digits.clear();
    }
}

fn format_mode_octal(mode: u32, format: PrintfFieldFormat) -> Vec<u8> {
    let mut digits = format!("{mode:o}").into_bytes();
    suppress_zero_digits(&mut digits, format);
    if format.alternate && !digits.starts_with(b"0") {
        digits.insert(0, b'0');
    }
    format_numeric_value(digits, None, format)
}

fn format_sparseness_ascii(size: u64, blocks: u64) -> String {
    if size == 0 {
        return "1".to_string();
    }

    let value = (blocks as f64 * 512.0) / size as f64;
    format_six_sigfigs_ascii(value)
}

fn format_six_sigfigs_ascii(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }

    let exponent = value.abs().log10().floor() as i32;
    if !(-4..6).contains(&exponent) {
        return trim_ascii_float(format!("{value:.5e}"));
    }

    let precision = (5 - exponent).max(0) as usize;
    trim_ascii_float(format!("{value:.precision$}", precision = precision))
}

fn trim_ascii_float(text: String) -> String {
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            format!("{}e{}", trim_ascii_decimal(mantissa), exponent)
        }
        None => trim_ascii_decimal(&text),
    }
}

fn trim_ascii_decimal(text: &str) -> String {
    let mut out = text.to_string();
    while out.contains('.') && out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
}

fn format_numeric_value(
    mut digits: Vec<u8>,
    sign: Option<u8>,
    format: PrintfFieldFormat,
) -> Vec<u8> {
    if let Some(precision) = format.precision {
        if digits.len() < precision {
            let mut prefixed = vec![b'0'; precision - digits.len()];
            prefixed.extend_from_slice(&digits);
            digits = prefixed;
        }
    }

    let mut value = Vec::with_capacity(digits.len() + usize::from(sign.is_some()));
    if let Some(sign) = sign {
        value.push(sign);
    }
    value.extend_from_slice(&digits);

    if format.left_align {
        return pad_field(&value, format.width, true, b' ');
    }

    let pad = if format.zero_pad && format.precision.is_none() {
        b'0'
    } else {
        b' '
    };

    if pad != b'0' || sign.is_none() {
        return pad_field(&value, format.width, false, pad);
    }

    let width = match format.width {
        Some(width) if width > value.len() => width,
        _ => return value,
    };

    let mut rendered = Vec::with_capacity(width);
    rendered.push(sign.unwrap());
    rendered.extend(std::iter::repeat_n(b'0', width - value.len()));
    rendered.extend_from_slice(&digits);
    rendered
}

fn file_type_letter(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::File => b'f',
        EntryKind::Directory => b'd',
        EntryKind::Symlink => b'l',
        EntryKind::Block => b'b',
        EntryKind::Character => b'c',
        EntryKind::Fifo => b'p',
        EntryKind::Socket => b's',
        EntryKind::Unknown => b'U',
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        PrintfAtom, PrintfDirective, PrintfDirectiveKind, PrintfFieldFormat, PrintfTimeFamily,
        PrintfTimeSelector, compile_printf_program, format_depth, format_mode_octal,
        format_sparseness_ascii, format_string_like, render_printf_bytes,
    };
    use crate::entry::EntryContext;
    use crate::eval::EvalContext;
    use crate::follow::FollowMode;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn compiler_accepts_the_full_stage_subset() {
        let program =
            compile_printf_program("-printf", OsStr::new("%p %P %f %h %d %y %s %m %l %%\\n"))
                .unwrap()
                .program;

        assert!(program.atoms.iter().any(|atom| matches!(
            atom,
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Path,
                ..
            })
        )));
        assert!(program.atoms.iter().any(|atom| matches!(
            atom,
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::RelativePath,
                ..
            })
        )));
        assert!(program.atoms.iter().any(|atom| matches!(
            atom,
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Basename,
                ..
            })
        )));
        assert!(program.atoms.iter().any(|atom| matches!(
            atom,
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Dirname,
                ..
            })
        )));
    }

    #[test]
    fn compiler_parses_supported_field_formatting() {
        let program =
            compile_printf_program("-printf", OsStr::new("[%10p][%-10p][%.3p][%010d][%#10m]"))
                .unwrap()
                .program;

        assert!(matches!(
            &program.atoms[1],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Path,
                format: PrintfFieldFormat {
                    width: Some(10),
                    precision: None,
                    left_align: false,
                    zero_pad: false,
                    always_sign: false,
                    space_sign: false,
                    alternate: false,
                },
            })
        ));
        assert!(matches!(
            &program.atoms[3],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Path,
                format: PrintfFieldFormat {
                    width: Some(10),
                    precision: None,
                    left_align: true,
                    ..
                },
            })
        ));
        assert!(matches!(
            &program.atoms[5],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Path,
                format: PrintfFieldFormat {
                    precision: Some(3),
                    ..
                },
            })
        ));
        assert!(matches!(
            &program.atoms[7],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::Depth,
                format: PrintfFieldFormat {
                    width: Some(10),
                    zero_pad: true,
                    ..
                },
            })
        ));
        assert!(matches!(
            &program.atoms[9],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::ModeOctal,
                format: PrintfFieldFormat {
                    width: Some(10),
                    alternate: true,
                    ..
                },
            })
        ));
    }

    #[test]
    fn compiler_rejects_malformed_field_formatting() {
        for (format, needle) in [
            ("%.", "malformed -printf format: expected digits after `.`"),
            (
                "%-.p",
                "malformed -printf format: expected digits after `.`",
            ),
            ("%10", "malformed -printf format: trailing %"),
        ] {
            let error = compile_printf_program("-printf", OsStr::new(format)).unwrap_err();
            assert!(
                error.message.contains(needle),
                "{format} -> {}",
                error.message
            );
        }
    }

    #[test]
    fn compiler_warns_about_unrecognized_directives_and_keeps_them_literal() {
        let compiled = compile_printf_program("-printf", OsStr::new("A%QB%5Q%08.3J\n")).unwrap();

        assert_eq!(
            compiled.warnings,
            vec![
                "rfd: warning: unrecognized format directive `%Q'".to_string(),
                "rfd: warning: unrecognized format directive `%Q'".to_string(),
                "rfd: warning: unrecognized format directive `%J'".to_string(),
            ]
        );

        let mut rendered = Vec::new();
        for atom in &compiled.program.atoms {
            match atom {
                PrintfAtom::Literal(bytes) => rendered.extend_from_slice(bytes),
                other => panic!("expected a literal atom, found {other:?}"),
            }
        }
        assert_eq!(rendered, b"A%QB%5Q%08.3J\n".to_vec());
    }

    #[test]
    fn compiler_treats_a_leading_zero_as_part_of_the_width() {
        let program = compile_printf_program("-printf", OsStr::new("[%08s][%0s][%8s][%-08s]"))
            .unwrap()
            .program;

        for (atom, width, zero_pad) in [
            (&program.atoms[1], 8, true),
            (&program.atoms[3], 0, true),
            (&program.atoms[5], 8, false),
            (&program.atoms[7], 8, true),
        ] {
            let PrintfAtom::Directive(PrintfDirective { format, .. }) = atom else {
                panic!("expected a directive, found {atom:?}");
            };
            assert_eq!(format.width, Some(width));
            assert_eq!(format.zero_pad, zero_pad);
        }

        assert!(
            compile_printf_program("-printf", OsStr::new("%0-8s")).is_ok_and(|compiled| {
                compiled
                    .warnings
                    .iter()
                    .any(|warning| warning.ends_with("unrecognized format directive `%-'"))
            })
        );
    }

    #[test]
    fn compiler_decodes_gnu_literal_escapes_and_collects_unknown_escape_warnings() {
        let compiled = compile_printf_program(
            "-printf",
            OsStr::new("A\\aB\\bC\\fD\\nE\\rF\\tG\\vH\\101\\040\\0123\\400\\q\\x"),
        )
        .unwrap();

        let literal_bytes = match &compiled.program.atoms[0] {
            PrintfAtom::Literal(bytes) => bytes.clone(),
            other => panic!("unexpected atom: {other:?}"),
        };

        assert_eq!(
            literal_bytes,
            b"A\x07B\x08C\x0cD\nE\rF\tG\x0bHA \n3\0\\q\\x".to_vec()
        );
        assert_eq!(compiled.warnings.len(), 2);
        assert_eq!(
            compiled.warnings,
            vec![
                "rfd: warning: unrecognized escape `\\q'".to_string(),
                "rfd: warning: unrecognized escape `\\x'".to_string(),
            ]
        );
    }

    #[test]
    fn compiler_emits_a_stop_atom_for_backslash_c() {
        let compiled = compile_printf_program("-printf", OsStr::new("A\\cB")).unwrap();
        assert!(matches!(compiled.program.atoms[0], PrintfAtom::Literal(_)));
        assert!(matches!(compiled.program.atoms[1], PrintfAtom::Stop));
    }

    #[test]
    fn format_sparseness_ascii_matches_gnu_host_samples() {
        assert_eq!(format_sparseness_ascii(0, 0), "1");
        assert_eq!(format_sparseness_ascii(1, 8), "4096");
        assert_eq!(format_sparseness_ascii(3, 8), "1365.33");
        assert_eq!(format_sparseness_ascii(5000, 16), "1.6384");
        assert_eq!(format_sparseness_ascii(8192, 8), "0.5");
        assert_eq!(format_sparseness_ascii(8192, 0), "0");
    }

    #[test]
    fn compiler_parses_full_and_family_time_directives() {
        let program = compile_printf_program(
            "-printf",
            OsStr::new("[%a][%c][%t][%B][%AY][%C@][%T+][%BY]"),
        )
        .unwrap()
        .program;

        assert!(matches!(
            &program.atoms[1],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Access),
                ..
            })
        ));
        assert!(matches!(
            &program.atoms[7],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Birth),
                ..
            })
        ));
        assert!(matches!(
            &program.atoms[9],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::TimestampPart {
                    family: PrintfTimeFamily::Access,
                    selector: PrintfTimeSelector::Byte(b'Y'),
                },
                ..
            })
        ));
        assert!(matches!(
            &program.atoms[11],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::TimestampPart {
                    family: PrintfTimeFamily::Change,
                    selector: PrintfTimeSelector::EpochSeconds,
                },
                ..
            })
        ));
    }

    #[test]
    fn compiler_treats_percent_b_without_a_selector_as_full_birth_time() {
        let program = compile_printf_program("-printf", OsStr::new("[%B][%BY]"))
            .unwrap()
            .program;

        assert!(matches!(
            &program.atoms[1],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Birth),
                ..
            })
        ));
        assert!(matches!(
            &program.atoms[3],
            PrintfAtom::Directive(PrintfDirective {
                kind: PrintfDirectiveKind::TimestampPart {
                    family: PrintfTimeFamily::Birth,
                    selector: PrintfTimeSelector::Byte(b'Y'),
                },
                ..
            })
        ));
    }

    #[test]
    fn compiler_warns_for_time_family_directives_that_end_the_format() {
        for (format, letter) in [("%A", "%A"), ("%C", "%C"), ("%T", "%T")] {
            let compiled = compile_printf_program("-printf", OsStr::new(format)).unwrap();

            assert_eq!(
                compiled.warnings,
                vec![format!(
                    "rfd: warning: format directive `{letter}' should be followed by another character"
                )],
                "{format}"
            );
            assert_eq!(
                compiled.program.atoms,
                vec![PrintfAtom::Literal(format.as_bytes().to_vec())],
                "{format}"
            );
        }
    }

    #[test]
    fn compiler_accepts_any_byte_as_a_time_selector() {
        let program = compile_printf_program("-printf", OsStr::new("[%TQ][%A~][%C7][%Ts][%Te]"))
            .unwrap()
            .program;

        for (atom, expected) in [
            (&program.atoms[1], b'Q'),
            (&program.atoms[3], b'~'),
            (&program.atoms[5], b'7'),
            (&program.atoms[7], b's'),
            (&program.atoms[9], b'e'),
        ] {
            assert!(
                matches!(
                    atom,
                    PrintfAtom::Directive(PrintfDirective {
                        kind: PrintfDirectiveKind::TimestampPart {
                            selector: PrintfTimeSelector::Byte(byte),
                            ..
                        },
                        ..
                    }) if *byte == expected
                ),
                "{expected} -> {atom:?}"
            );
        }
    }

    #[test]
    fn printf_with_fstype_directive_compiles() {
        let program = compile_printf_program("-printf", OsStr::new("%F"))
            .unwrap()
            .program;

        assert_eq!(program.atoms.len(), 1);
    }

    #[test]
    fn printf_with_fstype_requires_mount_snapshot_context() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "x").unwrap();
        let entry = EntryContext::new(path, 0, true);
        let program = compile_printf_program("-printf", OsStr::new("%F"))
            .unwrap()
            .program;

        let error = render_printf_bytes(
            &program,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
        )
        .unwrap_err();

        assert!(error.message.contains("mount snapshot"));
    }

    #[test]
    fn field_formatter_handles_string_and_numeric_directives() {
        assert_eq!(
            format_string_like(
                b"ext4",
                PrintfFieldFormat {
                    precision: Some(2),
                    ..PrintfFieldFormat::default()
                }
            ),
            b"ex"
        );
        let string_zero_pad = format_string_like(
            b"Mon",
            PrintfFieldFormat {
                width: Some(10),
                zero_pad: true,
                ..PrintfFieldFormat::default()
            },
        );
        let expected = if crate::platform::printf_zero_pads_string_fields() {
            b"0000000Mon".to_vec()
        } else {
            b"       Mon".to_vec()
        };
        assert_eq!(string_zero_pad, expected);
        assert_eq!(
            format_depth(
                0,
                PrintfFieldFormat {
                    always_sign: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            b"+0"
        );
        assert_eq!(
            format_mode_octal(
                0o664,
                PrintfFieldFormat {
                    alternate: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            b"0664"
        );
    }

    #[test]
    fn string_fields_zero_pad_through_a_precision_unless_left_aligned() {
        let prefixed = if crate::platform::printf_zero_pads_string_fields() {
            b"00000f1.".to_vec()
        } else {
            b"     f1.".to_vec()
        };
        assert_eq!(
            format_string_like(
                b"f1.txt",
                PrintfFieldFormat {
                    width: Some(8),
                    precision: Some(3),
                    zero_pad: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            prefixed
        );
        assert_eq!(
            format_string_like(
                b"f1.txt",
                PrintfFieldFormat {
                    width: Some(8),
                    precision: Some(3),
                    zero_pad: true,
                    left_align: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            b"f1.     "
        );
    }

    #[test]
    fn depth_honours_the_space_flag_and_drops_zero_digits_under_a_zero_precision() {
        let space = PrintfFieldFormat {
            space_sign: true,
            ..PrintfFieldFormat::default()
        };
        assert_eq!(format_depth(0, space), b" 0");
        assert_eq!(
            format_depth(
                0,
                PrintfFieldFormat {
                    width: Some(5),
                    zero_pad: true,
                    space_sign: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            b" 0000"
        );
        // A `+` takes precedence over the space flag.
        assert_eq!(
            format_depth(
                0,
                PrintfFieldFormat {
                    always_sign: true,
                    space_sign: true,
                    ..PrintfFieldFormat::default()
                }
            ),
            b"+0"
        );
        assert_eq!(
            format_depth(
                0,
                PrintfFieldFormat {
                    precision: Some(0),
                    ..PrintfFieldFormat::default()
                }
            ),
            b""
        );
        assert_eq!(
            format_depth(
                1,
                PrintfFieldFormat {
                    precision: Some(0),
                    ..PrintfFieldFormat::default()
                }
            ),
            b"1"
        );
    }

    #[test]
    fn render_printf_bytes_uses_empty_string_for_non_symlink_l() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let entry = EntryContext::new(path, 0, true);

        let program = compile_printf_program("-printf", OsStr::new("[%y][%s][%m][%l]"))
            .unwrap()
            .program;
        let rendered = render_printf_bytes(
            &program,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
        )
        .unwrap();

        assert_eq!(String::from_utf8(rendered).unwrap(), "[f][5][640][]");
    }

    #[test]
    fn render_printf_bytes_supports_identity_ownership_and_mode_directives() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(root.path().join("dir/file.txt"), "hello").unwrap();
        fs::set_permissions(
            root.path().join("dir/file.txt"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();

        let entry = EntryContext::with_file_type_hint_and_root(
            root.path().join("dir/file.txt"),
            1,
            false,
            Arc::new(root.path().to_path_buf()),
            None,
        );
        let program = compile_printf_program(
            "-printf",
            OsStr::new("[%H][%P][%i][%n][%D][%b][%k][%M][%u][%U][%g][%G]"),
        )
        .unwrap()
        .program;
        let rendered = render_printf_bytes(
            &program,
            &entry,
            FollowMode::Physical,
            &EvalContext::default(),
        )
        .unwrap();
        let text = String::from_utf8(rendered).unwrap();
        let metadata = fs::metadata(root.path().join("dir/file.txt")).unwrap();

        assert!(text.contains(&format!("[{}]", root.path().display())));
        assert!(text.contains("[dir/file.txt]"));
        assert!(text.contains(&format!("[{}]", metadata.ino())));
        assert!(text.contains(&format!("[{}]", metadata.nlink())));
        assert!(text.contains(&format!("[{}]", metadata.dev())));
        assert!(text.contains("[-rw-r-----]"));
        assert!(text.contains(&format!("[{}]", metadata.uid())));
        assert!(text.contains(&format!("[{}]", metadata.gid())));
    }
}
