use crate::account::{group_name, user_name};
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind, PrintfTargetKind};
use crate::eval::EvalContext;
use crate::follow::FollowMode;
use crate::printf_time::{
    ResolvedTimeParts, render_full_time_bytes, render_selector_bytes, resolve_local_time_parts,
};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

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
    GroupName,
    GroupId,
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

                index += 1;
                let directive = *bytes.get(index).ok_or_else(|| {
                    Diagnostic::new(format!("malformed {flag} format: trailing %"), 1)
                })?;

                if directive == b'%' {
                    atoms.push(PrintfAtom::Literal(vec![b'%']));
                } else {
                    atoms.push(PrintfAtom::Directive(parse_directive(
                        flag, bytes, &mut index,
                    )?));
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
                "findoxide: warning: unrecognized escape `\\{}'",
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

fn parse_directive(
    flag: &str,
    bytes: &[u8],
    index: &mut usize,
) -> Result<PrintfDirective, Diagnostic> {
    let mut format = PrintfFieldFormat::default();

    loop {
        match bytes.get(*index).copied() {
            Some(b'-') => format.left_align = true,
            Some(b'0') => format.zero_pad = true,
            Some(b'+') => format.always_sign = true,
            Some(b'#') => format.alternate = true,
            _ => break,
        }
        *index += 1;
    }

    format.width = parse_optional_usize(flag, bytes, index)?;
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
        b'B' => parse_birth_directive(flag, bytes, index)?,
        b'A' | b'C' | b'T' => parse_time_family_directive(flag, directive, bytes, index)?,
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
        b'U' => PrintfDirectiveKind::UserId,
        b'g' => PrintfDirectiveKind::GroupName,
        b'G' => PrintfDirectiveKind::GroupId,
        b'F' => PrintfDirectiveKind::FileSystemType,
        other => {
            return Err(Diagnostic::new(
                format!("unsupported {flag} directive %{}", char::from(other)),
                1,
            ));
        }
    };

    Ok(PrintfDirective { kind, format })
}

fn parse_birth_directive(
    flag: &str,
    bytes: &[u8],
    index: &mut usize,
) -> Result<PrintfDirectiveKind, Diagnostic> {
    match bytes.get(*index + 1).copied() {
        Some(next) if is_time_selector_lead_byte(next) => {
            let selector = parse_time_selector_byte(next).ok_or_else(|| {
                Diagnostic::new(
                    format!("unsupported {flag} time selector %B{}", char::from(next)),
                    1,
                )
            })?;
            *index += 1;
            Ok(PrintfDirectiveKind::TimestampPart {
                family: PrintfTimeFamily::Birth,
                selector,
            })
        }
        _ => Ok(PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Birth)),
    }
}

fn parse_time_family_directive(
    flag: &str,
    directive: u8,
    bytes: &[u8],
    index: &mut usize,
) -> Result<PrintfDirectiveKind, Diagnostic> {
    let family = match directive {
        b'A' => PrintfTimeFamily::Access,
        b'C' => PrintfTimeFamily::Change,
        b'T' => PrintfTimeFamily::Modification,
        _ => unreachable!("caller restricts directive"),
    };

    let selector_byte = bytes.get(*index + 1).copied().ok_or_else(|| {
        Diagnostic::new(
            format!(
                "malformed {flag} format: missing selector for %{}",
                char::from(directive)
            ),
            1,
        )
    })?;
    let selector = parse_time_selector_byte(selector_byte).ok_or_else(|| {
        Diagnostic::new(
            format!(
                "unsupported {flag} time selector %{}{}",
                char::from(directive),
                char::from(selector_byte)
            ),
            1,
        )
    })?;
    *index += 1;

    Ok(PrintfDirectiveKind::TimestampPart { family, selector })
}

fn is_time_selector_lead_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'@' | b'+')
}

fn parse_time_selector_byte(byte: u8) -> Option<PrintfTimeSelector> {
    match byte {
        b'@' => Some(PrintfTimeSelector::EpochSeconds),
        b'+' => Some(PrintfTimeSelector::GnuPlus),
        b'a' | b'A' | b'b' | b'B' | b'c' | b'd' | b'D' | b'F' | b'g' | b'G' | b'h' | b'H'
        | b'I' | b'j' | b'm' | b'M' | b'p' | b'r' | b'R' | b'S' | b't' | b'T' | b'u' | b'U'
        | b'V' | b'w' | b'W' | b'x' | b'X' | b'y' | b'Y' | b'z' | b'Z' => {
            Some(PrintfTimeSelector::Byte(byte))
        }
        _ => None,
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

fn render_directive_bytes(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    state: &mut PrintfRenderState,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
        PrintfDirectiveKind::Path => {
            format_string_like(entry.path.as_os_str().as_bytes(), directive.format)
        }
        PrintfDirectiveKind::RelativePath => format_string_like(
            entry.relative_to_root()?.as_os_str().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::StartPath => {
            format_string_like(entry.start_path().as_os_str().as_bytes(), directive.format)
        }
        PrintfDirectiveKind::Basename => format_string_like(
            entry
                .path
                .file_name()
                .unwrap_or_else(|| OsStr::new(""))
                .as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Dirname => format_string_like(
            entry.dirname_for_printf().as_os_str().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Depth => format_depth(entry.depth, directive.format),
        PrintfDirectiveKind::FileType => format_string_like(
            &[file_type_letter(entry.active_kind(follow_mode)?)],
            directive.format,
        ),
        PrintfDirectiveKind::FileTypeFollow => {
            let byte = match entry.printf_target_kind(follow_mode)? {
                PrintfTargetKind::Kind(kind) => file_type_letter(kind),
                PrintfTargetKind::Loop => b'L',
                PrintfTargetKind::Missing => b'N',
                PrintfTargetKind::OtherError => b'?',
            };
            format_string_like(&[byte], directive.format)
        }
        PrintfDirectiveKind::Size => format_string_like(
            entry.active_size(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Sparseness => {
            let text = format_sparseness_ascii(
                entry.active_size(follow_mode)?,
                entry.active_blocks(follow_mode)?,
            );
            format_string_like(text.as_bytes(), directive.format)
        }
        PrintfDirectiveKind::ModeOctal => {
            format_mode_octal(entry.active_mode_bits(follow_mode)?, directive.format)
        }
        PrintfDirectiveKind::ModeSymbolic => {
            let mode = symbolic_mode_string(
                entry.active_kind(follow_mode)?,
                entry.active_mode_bits(follow_mode)?,
            );
            format_string_like(mode.as_bytes(), directive.format)
        }
        PrintfDirectiveKind::LinkTarget => format_string_like(
            entry
                .active_link_target(follow_mode)?
                .as_deref()
                .unwrap_or_else(|| OsStr::new(""))
                .as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Inode => format_string_like(
            entry.active_inode(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::LinkCount => format_string_like(
            entry.active_link_count(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Device => format_string_like(
            entry.active_device(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Blocks512 => format_string_like(
            entry.active_blocks(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::Blocks1024 => {
            let blocks = entry.active_blocks(follow_mode)?;
            format_string_like(((blocks + 1) / 2).to_string().as_bytes(), directive.format)
        }
        PrintfDirectiveKind::UserName => {
            let uid = entry.active_uid(follow_mode)?;
            let name = user_name(uid)?;
            format_string_like(
                name_or_id_bytes(name.as_deref(), uid).as_slice(),
                directive.format,
            )
        }
        PrintfDirectiveKind::UserId => format_string_like(
            entry.active_uid(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::GroupName => {
            let gid = entry.active_gid(follow_mode)?;
            let name = group_name(gid)?;
            format_string_like(
                name_or_id_bytes(name.as_deref(), gid).as_slice(),
                directive.format,
            )
        }
        PrintfDirectiveKind::GroupId => format_string_like(
            entry.active_gid(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::FileSystemType => {
            let snapshot = context.mount_snapshot()?;
            let mount_id = entry.active_mount_id(follow_mode)?;
            let type_name = snapshot.type_for_mount_id(mount_id).ok_or_else(|| {
                Diagnostic::new(
                    format!("internal error: mount ID {mount_id} missing from mount snapshot"),
                    1,
                )
            })?;
            format_string_like(type_name.as_bytes(), directive.format)
        }
        PrintfDirectiveKind::FullTimestamp(family) => {
            match resolve_cached_time_parts(state, family, entry, follow_mode)? {
                Some(parts) => {
                    format_string_like(&render_full_time_bytes(parts)?, directive.format)
                }
                None => format_string_like(b"", directive.format),
            }
        }
        PrintfDirectiveKind::TimestampPart { family, selector } => {
            match resolve_cached_time_parts(state, family, entry, follow_mode)? {
                Some(parts) => {
                    format_string_like(&render_selector_bytes(parts, selector)?, directive.format)
                }
                None => format_string_like(b"", directive.format),
            }
        }
    })
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

fn name_or_id_bytes(name: Option<&OsStr>, id: u32) -> Vec<u8> {
    match name {
        Some(name) => name.as_bytes().to_vec(),
        None => id.to_string().into_bytes(),
    }
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
    pad_field(value, format.width, format.left_align, b' ')
}

fn format_depth(depth: usize, format: PrintfFieldFormat) -> Vec<u8> {
    let sign = if format.always_sign { Some(b'+') } else { None };
    format_numeric_value(depth.to_string().into_bytes(), sign, format)
}

fn format_mode_octal(mode: u32, format: PrintfFieldFormat) -> Vec<u8> {
    let mut digits = format!("{mode:o}").into_bytes();
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
    if exponent >= 6 || exponent < -4 {
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

fn symbolic_mode_string(kind: EntryKind, mode: u32) -> String {
    let mut value = String::with_capacity(10);
    value.push(match kind {
        EntryKind::File => '-',
        EntryKind::Directory => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::Block => 'b',
        EntryKind::Character => 'c',
        EntryKind::Fifo => 'p',
        EntryKind::Socket => 's',
        EntryKind::Unknown => 'U',
    });
    value.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    value.push(execute_char(mode, 0o100, 0o4000, 's', 'S'));
    value.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    value.push(execute_char(mode, 0o010, 0o2000, 's', 'S'));
    value.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    value.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    value.push(execute_char(mode, 0o001, 0o1000, 't', 'T'));
    value
}

fn execute_char(
    mode: u32,
    exec_bit: u32,
    special_bit: u32,
    when_set: char,
    when_unset: char,
) -> char {
    match (mode & exec_bit != 0, mode & special_bit != 0) {
        (true, true) => when_set,
        (false, true) => when_unset,
        (true, false) => 'x',
        (false, false) => '-',
    }
}

#[cfg(test)]
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
            ("%q", "unsupported -printf directive %q"),
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
                "findoxide: warning: unrecognized escape `\\q'".to_string(),
                "findoxide: warning: unrecognized escape `\\x'".to_string(),
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
    fn compiler_rejects_missing_or_unknown_time_selectors() {
        for (format, needle) in [
            ("%A", "missing selector for %A"),
            ("%C", "missing selector for %C"),
            ("%T", "missing selector for %T"),
            ("%Aq", "unsupported -printf time selector %Aq"),
            ("%T~", "unsupported -printf time selector %T~"),
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
