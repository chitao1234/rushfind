#[cfg(not(windows))]
use crate::account::group_name;
use crate::account::user_name;
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::eval::EvalContext;
use crate::follow::FollowMode;
use crate::metadata_format::name_or_id_bytes;
#[cfg(not(windows))]
use crate::metadata_format::symbolic_mode_string;
use crate::platform::path::{display_bytes, display_os_bytes};
use crate::printf_time::{ResolvedTimeParts, resolve_local_time_parts};
use crate::time::Timestamp;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_ENCRYPTED,
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SYSTEM,
};

const MONTHS_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];
const RECENT_PAST_WINDOW_SECONDS: i64 = 6 * 30 * 24 * 60 * 60;
const RECENT_FUTURE_WINDOW_SECONDS: i64 = 60 * 60;
#[cfg(not(windows))]
const OWNER_GROUP_FIELD_WIDTH: usize = 8;
#[cfg(windows)]
const WINDOWS_FILE_ID_FIELD_WIDTH: usize = 18;
#[cfg(windows)]
const WINDOWS_OWNER_FIELD_WIDTH: usize = 20;
const SIZE_FIELD_WIDTH: usize = 8;

pub(crate) fn render_ls_record(
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    #[cfg(windows)]
    {
        return render_windows_ls_record(entry, follow_mode, context);
    }

    #[cfg(not(windows))]
    {
        render_unix_ls_record(entry, follow_mode, context)
    }
}

#[cfg(not(windows))]
fn render_unix_ls_record(
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    let kind = entry.active_kind(follow_mode)?;
    let inode = entry.active_inode(follow_mode)?;
    let blocks_1k = entry.active_blocks(follow_mode)?.div_ceil(2);
    let mode = symbolic_mode_string(kind, entry.active_mode_bits(follow_mode)?);
    let links = entry.active_link_count(follow_mode)?;
    let owner_id = entry.active_owner(follow_mode)?;
    let group_id = entry.active_group(follow_mode)?;
    let owner = name_or_id_bytes(user_name(owner_id.clone())?.as_deref(), &owner_id);
    let group = name_or_id_bytes(group_name(group_id.clone())?.as_deref(), &group_id);
    let size = render_size_field(entry, follow_mode, kind)?;
    let timestamp = render_entry_timestamp(entry, follow_mode, context.evaluation_now()?)?;
    let path = escape_ls_name_bytes(&display_bytes(&entry.path));
    let suffix = render_symlink_suffix(entry, follow_mode)?;

    let mut out = format!("{inode:>9} {blocks_1k:>6} {mode} {links:>3} ").into_bytes();
    append_padded_field(&mut out, &owner, OWNER_GROUP_FIELD_WIDTH);
    out.push(b' ');
    append_padded_field(&mut out, &group, OWNER_GROUP_FIELD_WIDTH);
    out.push(b' ');
    out.extend_from_slice(&pad_left(&size, SIZE_FIELD_WIDTH));
    out.push(b' ');
    out.extend_from_slice(&timestamp);
    out.push(b' ');
    out.extend_from_slice(&path);
    out.extend_from_slice(&suffix);
    out.push(b'\n');
    Ok(out)
}

#[cfg(windows)]
fn render_windows_ls_record(
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    let kind = entry.active_kind(follow_mode)?;
    let fileid = entry.active_inode(follow_mode)?.to_string().into_bytes();
    let alloc_kib = allocation_kib_bytes(entry.active_allocation_size(follow_mode)?);
    let attrs = windows_attribute_string(kind, entry.active_native_attributes(follow_mode)?);
    let links = entry
        .active_link_count(follow_mode)?
        .to_string()
        .into_bytes();
    let owner_id = entry.active_owner(follow_mode)?;
    let owner = name_or_id_bytes(user_name(owner_id.clone())?.as_deref(), &owner_id);
    let size = entry.active_size(follow_mode)?.to_string().into_bytes();
    let timestamp = render_entry_timestamp(entry, follow_mode, context.evaluation_now()?)?;
    let path = escape_ls_bytes(&display_bytes(&entry.path));
    let suffix = render_symlink_suffix(entry, follow_mode)?;

    let mut out = pad_left(&fileid, WINDOWS_FILE_ID_FIELD_WIDTH);
    out.push(b' ');
    out.extend_from_slice(&pad_left(&alloc_kib, SIZE_FIELD_WIDTH));
    out.push(b' ');
    out.extend_from_slice(attrs.as_bytes());
    out.push(b' ');
    out.extend_from_slice(&pad_left(&links, 3));
    out.push(b' ');
    append_padded_field(&mut out, &owner, WINDOWS_OWNER_FIELD_WIDTH);
    out.push(b' ');
    out.extend_from_slice(&pad_left(&size, SIZE_FIELD_WIDTH));
    out.push(b' ');
    out.extend_from_slice(&timestamp);
    out.push(b' ');
    out.extend_from_slice(&path);
    out.extend_from_slice(&suffix);
    out.push(b'\n');
    Ok(out)
}

fn render_entry_timestamp(
    entry: &EntryContext,
    follow_mode: FollowMode,
    now: Timestamp,
) -> Result<Vec<u8>, Diagnostic> {
    let mtime = entry.active_mtime(follow_mode)?;
    let parts = resolve_local_time_parts(mtime)?;
    render_ls_time_column(&parts, now)
}

#[cfg(not(windows))]
fn render_size_field(
    entry: &EntryContext,
    follow_mode: FollowMode,
    kind: EntryKind,
) -> Result<Vec<u8>, Diagnostic> {
    match (kind, entry.active_device_number(follow_mode)?) {
        #[cfg(unix)]
        (EntryKind::Block | EntryKind::Character, Some(device)) => {
            let major = device_major(device as libc::dev_t);
            let minor = device_minor(device as libc::dev_t);
            Ok(format_device_field(major, minor))
        }
        _ => Ok(entry.active_size(follow_mode)?.to_string().into_bytes()),
    }
}

#[cfg(windows)]
fn allocation_kib_bytes(allocation_size: u64) -> Vec<u8> {
    allocation_size.div_ceil(1024).to_string().into_bytes()
}

#[cfg(all(unix, any(target_os = "solaris", target_os = "illumos")))]
fn device_major(device: libc::dev_t) -> u64 {
    unsafe { libc::major(device) as u64 }
}

#[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
fn device_major(device: libc::dev_t) -> u64 {
    libc::major(device) as u64
}

#[cfg(all(unix, any(target_os = "solaris", target_os = "illumos")))]
fn device_minor(device: libc::dev_t) -> u64 {
    unsafe { libc::minor(device) as u64 }
}

#[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
fn device_minor(device: libc::dev_t) -> u64 {
    libc::minor(device) as u64
}

fn render_symlink_suffix(
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    match entry.active_link_target(follow_mode)? {
        Some(target) => {
            let mut bytes = b" -> ".to_vec();
            bytes.extend_from_slice(&escape_ls_name_bytes(&display_os_bytes(target.as_os_str())));
            Ok(bytes)
        }
        None => Ok(Vec::new()),
    }
}

fn append_padded_field(out: &mut Vec<u8>, value: &[u8], width: usize) {
    out.extend_from_slice(value);
    if value.len() < width {
        out.extend(std::iter::repeat_n(b' ', width - value.len()));
    }
}

fn pad_left(value: &[u8], width: usize) -> Vec<u8> {
    if value.len() >= width {
        return value.to_vec();
    }

    let mut out = Vec::with_capacity(width);
    out.extend(std::iter::repeat_n(b' ', width - value.len()));
    out.extend_from_slice(value);
    out
}

#[cfg(windows)]
fn windows_attribute_string(kind: EntryKind, attributes: u32) -> String {
    let mut value = String::with_capacity(8);
    value.push(match kind {
        EntryKind::File => '-',
        EntryKind::Directory => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::Block => 'b',
        EntryKind::Character => 'c',
        EntryKind::Fifo => 'p',
        EntryKind::Socket => 's',
        EntryKind::Unknown => '?',
    });
    value.push(if attributes & FILE_ATTRIBUTE_READONLY != 0 {
        'R'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
        'H'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_SYSTEM != 0 {
        'S'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_ARCHIVE != 0 {
        'A'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_COMPRESSED != 0 {
        'C'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_ENCRYPTED != 0 {
        'E'
    } else {
        '-'
    });
    value.push(if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        'P'
    } else {
        '-'
    });
    value
}

fn recent_window_contains(now: Timestamp, timestamp: Timestamp) -> bool {
    let not_too_old = match now.seconds.checked_sub(RECENT_PAST_WINDOW_SECONDS) {
        Some(seconds) => timestamp >= Timestamp::new(seconds, now.nanos),
        None => true,
    };
    let not_too_future = match now.seconds.checked_add(RECENT_FUTURE_WINDOW_SECONDS) {
        Some(seconds) => timestamp <= Timestamp::new(seconds, now.nanos),
        None => true,
    };
    not_too_old && not_too_future
}

fn render_ls_time_column(parts: &ResolvedTimeParts, now: Timestamp) -> Result<Vec<u8>, Diagnostic> {
    let month = MONTHS_ABBR
        .get(parts.local.tm_mon as usize)
        .ok_or_else(|| Diagnostic::new("internal error: local month out of range", 1))?;

    if recent_window_contains(now, parts.timestamp) {
        Ok(format!(
            "{} {:>2} {:02}:{:02}",
            std::str::from_utf8(month).unwrap(),
            parts.local.tm_mday,
            parts.local.tm_hour,
            parts.local.tm_min
        )
        .into_bytes())
    } else {
        Ok(format!(
            "{} {:>2}  {:04}",
            std::str::from_utf8(month).unwrap(),
            parts.local.tm_mday,
            parts.local.tm_year + 1900
        )
        .into_bytes())
    }
}

#[cfg(not(windows))]
fn format_device_field(major: u64, minor: u64) -> Vec<u8> {
    format!("{major}, {minor:>3}").into_bytes()
}

/// GNU `find -ls` escapes the printed name and the symlink target one byte at a
/// time, with a rule that does not consult the locale: a letter escape for the
/// backspace, tab, newline, form feed and carriage return, `\\` for a
/// backslash, a backslash in front of a space or a double quote, printable
/// ASCII as itself, and a three-digit octal `\NNN` for everything else. The
/// bell and the vertical tab have no letter escape here, and a multibyte
/// character is escaped byte by byte. That is `print_name_with_quoting` in
/// findutils' `lib/listfile.c`.
#[cfg(not(windows))]
fn escape_ls_name_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for byte in bytes {
        // `"` and `\` are inside the printable range, so they come first.
        match *byte {
            b'\\' => out.extend_from_slice(br"\\"),
            b'\n' => out.extend_from_slice(br"\n"),
            b'\x08' => out.extend_from_slice(br"\b"),
            b'\r' => out.extend_from_slice(br"\r"),
            b'\t' => out.extend_from_slice(br"\t"),
            b'\x0c' => out.extend_from_slice(br"\f"),
            b' ' => out.extend_from_slice(br"\ "),
            b'"' => out.extend_from_slice(br#"\""#),
            0x21..=0x7e => out.push(*byte),
            other => {
                out.push(b'\\');
                out.push(b'0' + (other >> 6));
                out.push(b'0' + ((other >> 3) & 0x07));
                out.push(b'0' + (other & 0x07));
            }
        }
    }
    out
}

/// The Windows record keeps its own printable-subset escaping, so the shared
/// symlink suffix still renders the way it did before GNU parity landed.
#[cfg(windows)]
fn escape_ls_name_bytes(bytes: &[u8]) -> Vec<u8> {
    escape_ls_bytes(bytes)
}

#[cfg(windows)]
fn escape_ls_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for byte in bytes {
        match byte {
            b' ' => out.extend_from_slice(br"\ "),
            b'\\' => out.extend_from_slice(br"\\"),
            b'\t' => out.extend_from_slice(br"\t"),
            b'\n' => out.extend_from_slice(br"\n"),
            b'\r' => out.extend_from_slice(br"\r"),
            0x0c => out.extend_from_slice(br"\f"),
            other => out.push(*other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::allocation_kib_bytes;
    #[cfg(windows)]
    use super::escape_ls_bytes;
    #[cfg(not(windows))]
    use super::escape_ls_name_bytes;
    #[cfg(not(windows))]
    use super::format_device_field;
    use super::{recent_window_contains, render_ls_time_column};
    #[cfg(not(windows))]
    use crate::account::PrincipalId;
    #[cfg(not(windows))]
    use crate::metadata_format::name_or_id_bytes;
    use crate::printf_time::ResolvedTimeParts;
    use crate::time::Timestamp;

    #[test]
    fn recent_window_is_inclusive_at_both_edges() {
        let now = Timestamp::new(1_700_000_000, 0);
        assert!(recent_window_contains(
            now,
            Timestamp::new(1_700_000_000 - 15_552_000, 0)
        ));
        assert!(recent_window_contains(
            now,
            Timestamp::new(1_700_000_000 + 3_600, 0)
        ));
    }

    #[test]
    fn recent_window_excludes_entries_just_outside_bounds() {
        let now = Timestamp::new(1_700_000_000, 0);
        assert!(!recent_window_contains(
            now,
            Timestamp::new(1_700_000_000 - 15_552_001, 0)
        ));
        assert!(!recent_window_contains(
            now,
            Timestamp::new(1_700_000_000 + 3_601, 0)
        ));
    }

    #[test]
    #[cfg(not(windows))]
    fn escape_ls_name_bytes_matches_the_gnu_rule() {
        // The named escapes, a space, a double quote, then a backslash pair:
        // `|` separators keep the escapes readable.
        assert_eq!(
            escape_ls_name_bytes(b" a|\t|\n|\r|\x08|\x0c|\\\\|\"|\\"),
            br#"\ a|\t|\n|\r|\b|\f|\\\\|\"|\\"#
        );
        // Control bytes without a C name are three-digit octal, not `\v`/`\a`.
        assert_eq!(escape_ls_name_bytes(b"\x0b"), br"\013");
        assert_eq!(escape_ls_name_bytes(b"\x07"), br"\007");
        assert_eq!(escape_ls_name_bytes(b"\x7f"), br"\177");
        assert_eq!(escape_ls_name_bytes(b"\x01"), br"\001");
        // Every byte of a valid UTF-8 sequence is escaped on its own.
        assert_eq!(
            escape_ls_name_bytes(b"\xe6\x97\xa5.txt"),
            br"\346\227\245.txt"
        );
        // Printable ASCII, including the shell metacharacters, is untouched.
        assert_eq!(
            escape_ls_name_bytes(b"!~-*?[]#&`|;$%^()'"),
            b"!~-*?[]#&`|;$%^()'"
        );
    }

    #[test]
    #[cfg(windows)]
    fn escape_ls_bytes_keeps_the_windows_printable_subset() {
        assert_eq!(
            escape_ls_bytes(b" a\tb\nc\rd\x0ce\\\\"),
            br"\ a\tb\nc\rd\fe\\\\"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn owner_group_fallback_uses_decimal_ids() {
        assert_eq!(name_or_id_bytes(None, &PrincipalId::Numeric(1234)), b"1234");
    }

    #[cfg(not(windows))]
    #[test]
    fn device_size_slot_renders_major_and_minor() {
        assert_eq!(format_device_field(1, 3), b"1,   3");
    }

    #[cfg(windows)]
    #[test]
    fn allocation_kib_rounds_up_to_whole_kib() {
        assert_eq!(allocation_kib_bytes(0), b"0");
        assert_eq!(allocation_kib_bytes(1), b"1");
        assert_eq!(allocation_kib_bytes(1024), b"1");
        assert_eq!(allocation_kib_bytes(1025), b"2");
    }

    #[test]
    fn time_column_switches_shape_at_the_recent_boundary() {
        let now = Timestamp::new(1_700_000_000, 0);
        let recent = ResolvedTimeParts::for_tests(
            Timestamp::new(1_700_000_000 - 60, 0),
            2023,
            11,
            14,
            22,
            12,
            20,
            2,
            318,
            0,
            28_800,
            b"CST".to_vec(),
        );
        let old = ResolvedTimeParts::for_tests(
            Timestamp::new(1_700_000_000 - 15_552_001, 0),
            2023,
            5,
            18,
            8,
            0,
            0,
            4,
            138,
            0,
            28_800,
            b"CST".to_vec(),
        );

        assert_eq!(
            render_ls_time_column(&recent, now).unwrap(),
            b"Nov 14 22:12"
        );
        assert_eq!(render_ls_time_column(&old, now).unwrap(), b"May 18  2023");
    }
}
