use crate::account::{group_name, user_name};
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, EntryKind};
use crate::eval::EvalContext;
use crate::follow::FollowMode;
use crate::printf_time::{ResolvedTimeParts, resolve_local_time_parts};
use crate::time::Timestamp;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

const MONTHS_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];
const RECENT_PAST_WINDOW_SECONDS: i64 = 6 * 30 * 24 * 60 * 60;
const RECENT_FUTURE_WINDOW_SECONDS: i64 = 60 * 60;
#[allow(dead_code)]
const OWNER_GROUP_FIELD_WIDTH: usize = 8;
#[allow(dead_code)]
const SIZE_FIELD_WIDTH: usize = 8;

#[allow(dead_code)]
pub(crate) fn render_ls_record(
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    let kind = entry.active_kind(follow_mode)?;
    let inode = entry.active_inode(follow_mode)?;
    let blocks_1k = entry.active_blocks(follow_mode)?.div_ceil(2);
    let mode = symbolic_mode_string(kind, entry.active_mode_bits(follow_mode)?);
    let links = entry.active_link_count(follow_mode)?;
    let uid = entry.active_uid(follow_mode)?;
    let gid = entry.active_gid(follow_mode)?;
    let owner = format_name_or_id(user_name(uid)?.as_deref(), uid);
    let group = format_name_or_id(group_name(gid)?.as_deref(), gid);
    let size = render_size_field(entry, follow_mode, kind)?;
    let timestamp = render_entry_timestamp(entry, follow_mode, context.evaluation_now()?)?;
    let path = escape_ls_bytes(entry.path.as_os_str().as_bytes());
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

#[allow(dead_code)]
fn render_entry_timestamp(
    entry: &EntryContext,
    follow_mode: FollowMode,
    now: Timestamp,
) -> Result<Vec<u8>, Diagnostic> {
    let mtime = entry.active_mtime(follow_mode)?;
    let parts = resolve_local_time_parts(mtime)?;
    render_ls_time_column(&parts, now)
}

#[allow(dead_code)]
fn render_size_field(
    entry: &EntryContext,
    follow_mode: FollowMode,
    kind: EntryKind,
) -> Result<Vec<u8>, Diagnostic> {
    match (kind, entry.active_device_number(follow_mode)?) {
        (EntryKind::Block | EntryKind::Character, Some(device)) => {
            let major = libc::major(device as libc::dev_t) as u64;
            let minor = libc::minor(device as libc::dev_t) as u64;
            Ok(format_device_field(major, minor))
        }
        _ => Ok(entry.active_size(follow_mode)?.to_string().into_bytes()),
    }
}

#[allow(dead_code)]
fn render_symlink_suffix(
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    match entry.active_link_target(follow_mode)? {
        Some(target) => {
            let mut bytes = b" -> ".to_vec();
            bytes.extend_from_slice(&escape_ls_bytes(target.as_bytes()));
            Ok(bytes)
        }
        None => Ok(Vec::new()),
    }
}

#[allow(dead_code)]
fn append_padded_field(out: &mut Vec<u8>, value: &[u8], width: usize) {
    out.extend_from_slice(value);
    if value.len() < width {
        out.extend(std::iter::repeat_n(b' ', width - value.len()));
    }
}

#[allow(dead_code)]
fn pad_left(value: &[u8], width: usize) -> Vec<u8> {
    if value.len() >= width {
        return value.to_vec();
    }

    let mut out = Vec::with_capacity(width);
    out.extend(std::iter::repeat_n(b' ', width - value.len()));
    out.extend_from_slice(value);
    out
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

fn format_name_or_id(name: Option<&OsStr>, id: u32) -> Vec<u8> {
    match name {
        Some(name) => name.as_bytes().to_vec(),
        None => id.to_string().into_bytes(),
    }
}

fn format_device_field(major: u64, minor: u64) -> Vec<u8> {
    format!("{major}, {minor:>3}").into_bytes()
}

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
    use super::{
        escape_ls_bytes, format_device_field, format_name_or_id, recent_window_contains,
        render_ls_time_column,
    };
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
    fn escape_ls_bytes_matches_the_gnu_subset_for_paths_and_targets() {
        assert_eq!(
            escape_ls_bytes(b" a\tb\nc\rd\x0ce\\\\"),
            br"\ a\tb\nc\rd\fe\\\\"
        );
    }

    #[test]
    fn owner_group_fallback_uses_decimal_ids() {
        assert_eq!(format_name_or_id(None, 1234), b"1234");
    }

    #[test]
    fn device_size_slot_renders_major_and_minor() {
        assert_eq!(format_device_field(1, 3), b"1,   3");
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
