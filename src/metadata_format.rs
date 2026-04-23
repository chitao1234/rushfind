use crate::account::PrincipalId;
use crate::entry::EntryKind;
use crate::platform::path::encoded_bytes;
use std::ffi::OsStr;

pub(crate) fn principal_id_bytes(id: &PrincipalId) -> Vec<u8> {
    match id {
        PrincipalId::Numeric(value) => value.to_string().into_bytes(),
        PrincipalId::Sid(value) => value.as_bytes().to_vec(),
    }
}

pub(crate) fn name_or_id_bytes(name: Option<&OsStr>, id: &PrincipalId) -> Vec<u8> {
    match name {
        Some(name) => encoded_bytes(name).to_vec(),
        None => principal_id_bytes(id),
    }
}

pub(crate) fn symbolic_mode_string(kind: EntryKind, mode: u32) -> String {
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
    use super::{name_or_id_bytes, principal_id_bytes, symbolic_mode_string};
    use crate::account::PrincipalId;
    use crate::entry::EntryKind;

    #[test]
    fn principal_helpers_preserve_numeric_and_sid_fallbacks() {
        assert_eq!(principal_id_bytes(&PrincipalId::Numeric(1234)), b"1234");
        assert_eq!(
            principal_id_bytes(&PrincipalId::Sid("S-1-5-18".into())),
            b"S-1-5-18"
        );
        assert_eq!(name_or_id_bytes(None, &PrincipalId::Numeric(1234)), b"1234");
    }

    #[test]
    fn symbolic_mode_string_renders_posix_shapes() {
        assert_eq!(symbolic_mode_string(EntryKind::File, 0o640), "-rw-r-----");
        assert_eq!(
            symbolic_mode_string(EntryKind::Directory, 0o1755),
            "drwxr-xr-t"
        );
    }
}
