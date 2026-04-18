use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::follow::FollowMode;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfProgram {
    pub atoms: Vec<PrintfAtom>,
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
pub enum PrintfDirectiveKind {
    Path,
    RelativePath,
    Basename,
    Dirname,
    Depth,
    FileType,
    Size,
    ModeOctal,
    LinkTarget,
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
}

pub fn compile_printf_program(flag: &str, format: &OsStr) -> Result<PrintfProgram, Diagnostic> {
    let bytes = format.as_encoded_bytes();
    let mut atoms = Vec::new();
    let mut literal = Vec::new();
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
                let escaped = *bytes.get(index).ok_or_else(|| {
                    Diagnostic::new(format!("malformed {flag} format: trailing \\"), 1)
                })?;

                literal.push(match escaped {
                    b'\\' => b'\\',
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'0' => b'\0',
                    other => {
                        return Err(Diagnostic::new(
                            format!(
                                "malformed {flag} format: unsupported escape \\{}",
                                char::from(other)
                            ),
                            1,
                        ));
                    }
                });
            }
            byte => literal.push(byte),
        }

        index += 1;
    }

    if !literal.is_empty() {
        atoms.push(PrintfAtom::Literal(literal));
    }

    Ok(PrintfProgram { atoms })
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

    let directive = *bytes.get(*index).ok_or_else(|| {
        Diagnostic::new(format!("malformed {flag} format: trailing %"), 1)
    })?;

    let kind = match directive {
        b'p' => PrintfDirectiveKind::Path,
        b'P' => PrintfDirectiveKind::RelativePath,
        b'f' => PrintfDirectiveKind::Basename,
        b'h' => PrintfDirectiveKind::Dirname,
        b'd' => PrintfDirectiveKind::Depth,
        b'y' => PrintfDirectiveKind::FileType,
        b's' => PrintfDirectiveKind::Size,
        b'm' => PrintfDirectiveKind::ModeOctal,
        b'l' => PrintfDirectiveKind::LinkTarget,
        other => {
            return Err(Diagnostic::new(
                format!("unsupported {flag} directive %{}", char::from(other)),
                1,
            ));
        }
    };

    Ok(PrintfDirective { kind, format })
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

pub fn render_printf_bytes(
    program: &PrintfProgram,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    let mut rendered = Vec::new();

    for atom in &program.atoms {
        match atom {
            PrintfAtom::Literal(bytes) => rendered.extend_from_slice(bytes),
            PrintfAtom::Directive(directive) => {
                rendered.extend_from_slice(&render_directive_bytes(
                    directive,
                    entry,
                    follow_mode,
                )?)
            }
        }
    }

    Ok(rendered)
}

fn render_directive_bytes(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
        PrintfDirectiveKind::Path => {
            format_string_like(entry.path.as_os_str().as_bytes(), directive.format)
        }
        PrintfDirectiveKind::RelativePath => {
            format_string_like(entry.relative_to_root()?.as_os_str().as_bytes(), directive.format)
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
        PrintfDirectiveKind::FileType => {
            format_string_like(&[file_type_letter(entry.active_kind(follow_mode)?)], directive.format)
        }
        PrintfDirectiveKind::Size => format_string_like(
            entry.active_size(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::ModeOctal => {
            format_mode_octal(entry.active_mode_bits(follow_mode)?, directive.format)
        }
        PrintfDirectiveKind::LinkTarget => format_string_like(
            entry
                .active_link_target(follow_mode)?
                .as_deref()
                .unwrap_or_else(|| OsStr::new(""))
                .as_bytes(),
            directive.format,
        ),
    })
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

#[cfg(test)]
mod tests {
    use super::{
        PrintfAtom, PrintfDirective, PrintfDirectiveKind, PrintfFieldFormat,
        compile_printf_program, format_depth, format_mode_octal, format_string_like,
        render_printf_bytes,
    };
    use crate::entry::EntryContext;
    use crate::follow::FollowMode;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn compiler_accepts_the_full_stage_subset() {
        let program =
            compile_printf_program("-printf", OsStr::new("%p %P %f %h %d %y %s %m %l %%\\n"))
                .unwrap();

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
        let program = compile_printf_program(
            "-printf",
            OsStr::new("[%10p][%-10p][%.3p][%010d][%#10m]"),
        )
        .unwrap();

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
            ("%-.p", "malformed -printf format: expected digits after `.`"),
            ("%10", "malformed -printf format: trailing %"),
            ("%q", "unsupported -printf directive %q"),
        ] {
            let error = compile_printf_program("-printf", OsStr::new(format)).unwrap_err();
            assert!(error.message.contains(needle), "{format} -> {}", error.message);
        }
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

        let program = compile_printf_program("-printf", OsStr::new("[%y][%s][%m][%l]")).unwrap();
        let rendered = render_printf_bytes(&program, &entry, FollowMode::Physical).unwrap();

        assert_eq!(String::from_utf8(rendered).unwrap(), "[f][5][640][]");
    }
}
