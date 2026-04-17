use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
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
    FileType,
    Size,
    ModeOctal,
    LinkTarget,
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
                    b'y' => PrintfAtom::FileType,
                    b's' => PrintfAtom::Size,
                    b'm' => PrintfAtom::ModeOctal,
                    b'l' => PrintfAtom::LinkTarget,
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
    follow_mode: FollowMode,
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
            PrintfAtom::FileType => {
                rendered.push(file_type_letter(entry.active_kind(follow_mode)?))
            }
            PrintfAtom::Size => {
                rendered.extend_from_slice(entry.active_size(follow_mode)?.to_string().as_bytes())
            }
            PrintfAtom::ModeOctal => rendered.extend_from_slice(
                format!("{:o}", entry.active_mode_bits(follow_mode)?).as_bytes(),
            ),
            PrintfAtom::LinkTarget => {
                if let Some(target) = entry.active_link_target(follow_mode)? {
                    rendered.extend_from_slice(target.as_bytes());
                }
            }
        }
    }

    Ok(rendered)
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
    use super::{PrintfAtom, compile_printf_program, render_printf_bytes};
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

        assert!(program.atoms.contains(&PrintfAtom::Path));
        assert!(program.atoms.contains(&PrintfAtom::RelativePath));
        assert!(program.atoms.contains(&PrintfAtom::Basename));
        assert!(program.atoms.contains(&PrintfAtom::Dirname));
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
