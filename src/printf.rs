use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::follow::FollowMode;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintfProgram {
    pub atoms: Vec<PrintfAtom>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintfAtom {
    Literal(Vec<u8>),
    Path,
    RelativePath,
    Basename,
    Dirname,
    Depth,
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

                atoms.push(match directive {
                    b'%' => PrintfAtom::Literal(vec![b'%']),
                    b'p' => PrintfAtom::Path,
                    b'P' => PrintfAtom::RelativePath,
                    b'f' => PrintfAtom::Basename,
                    b'h' => PrintfAtom::Dirname,
                    b'd' => PrintfAtom::Depth,
                    other => {
                        return Err(Diagnostic::new(
                            format!("unsupported {flag} directive %{}", char::from(other)),
                            1,
                        ));
                    }
                });
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

pub fn render_printf_bytes(
    program: &PrintfProgram,
    entry: &EntryContext,
    _follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    let mut rendered = Vec::new();

    for atom in &program.atoms {
        match atom {
            PrintfAtom::Literal(bytes) => rendered.extend_from_slice(bytes),
            PrintfAtom::Path => rendered.extend_from_slice(entry.path.as_os_str().as_bytes()),
            PrintfAtom::RelativePath => {
                rendered.extend_from_slice(entry.relative_to_root()?.as_os_str().as_bytes())
            }
            PrintfAtom::Basename => rendered.extend_from_slice(
                entry
                    .path
                    .file_name()
                    .unwrap_or_else(|| OsStr::new(""))
                    .as_bytes(),
            ),
            PrintfAtom::Dirname => {
                rendered.extend_from_slice(entry.dirname_for_printf().as_os_str().as_bytes())
            }
            PrintfAtom::Depth => rendered.extend_from_slice(entry.depth.to_string().as_bytes()),
        }
    }

    Ok(rendered)
}
