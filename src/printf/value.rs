use crate::account::{group_name, user_name};
use crate::diagnostics::Diagnostic;
use crate::entry::{EntryContext, PrintfTargetKind};
use crate::eval::EvalContext;
use crate::follow::FollowMode;
use crate::metadata_format::{name_or_id_bytes, principal_id_bytes, symbolic_mode_string};
use crate::platform::path::{
    display_bytes, display_os_bytes, encoded_bytes, relative_dir_for_printf,
};
use crate::printf_time::{render_full_time_bytes, render_selector_bytes};
use std::ffi::OsStr;

use super::{
    PrintfDirective, PrintfDirectiveKind, PrintfRenderState, PrintfTimeSelector, file_type_letter,
    format_depth, format_mode_octal, format_sparseness_ascii, format_string_like,
    resolve_cached_time_parts,
};

pub(super) fn render_directive_bytes(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    state: &mut PrintfRenderState,
) -> Result<Vec<u8>, Diagnostic> {
    match directive.kind {
        PrintfDirectiveKind::Path
        | PrintfDirectiveKind::RelativePath
        | PrintfDirectiveKind::StartPath
        | PrintfDirectiveKind::Basename
        | PrintfDirectiveKind::Dirname => render_path_directive(directive, entry),
        PrintfDirectiveKind::Depth
        | PrintfDirectiveKind::FileType
        | PrintfDirectiveKind::FileTypeFollow => {
            render_shape_directive(directive, entry, follow_mode)
        }
        PrintfDirectiveKind::Size
        | PrintfDirectiveKind::Sparseness
        | PrintfDirectiveKind::ModeOctal
        | PrintfDirectiveKind::ModeSymbolic
        | PrintfDirectiveKind::LinkTarget
        | PrintfDirectiveKind::Inode
        | PrintfDirectiveKind::LinkCount
        | PrintfDirectiveKind::Device
        | PrintfDirectiveKind::Blocks512
        | PrintfDirectiveKind::Blocks1024 => {
            render_metadata_directive(directive, entry, follow_mode)
        }
        PrintfDirectiveKind::UserName
        | PrintfDirectiveKind::UserSid
        | PrintfDirectiveKind::UserId
        | PrintfDirectiveKind::GroupName
        | PrintfDirectiveKind::GroupSid
        | PrintfDirectiveKind::GroupId => render_principal_directive(directive, entry, follow_mode),
        PrintfDirectiveKind::FileSystemType => {
            render_filesystem_type_directive(directive, entry, follow_mode, context)
        }
        PrintfDirectiveKind::FullTimestamp(_) | PrintfDirectiveKind::TimestampPart { .. } => {
            render_time_directive(directive, entry, follow_mode, state)
        }
    }
}

fn render_path_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
        PrintfDirectiveKind::Path => {
            format_string_like(&display_bytes(&entry.path), directive.format)
        }
        PrintfDirectiveKind::RelativePath => {
            format_string_like(&display_bytes(entry.relative_to_root()?), directive.format)
        }
        PrintfDirectiveKind::StartPath => {
            format_string_like(&display_bytes(entry.start_path()), directive.format)
        }
        PrintfDirectiveKind::Basename => format_string_like(
            &display_os_bytes(entry.path.file_name().unwrap_or_else(|| OsStr::new(""))),
            directive.format,
        ),
        PrintfDirectiveKind::Dirname => format_string_like(
            &display_bytes(relative_dir_for_printf(&entry.path).as_path()),
            directive.format,
        ),
        _ => unreachable!("directive dispatch guarantees path directive"),
    })
}

fn render_shape_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
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
        _ => unreachable!("directive dispatch guarantees shape directive"),
    })
}

fn render_metadata_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
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
            &display_os_bytes(
                entry
                    .active_link_target(follow_mode)?
                    .as_deref()
                    .unwrap_or_else(|| OsStr::new("")),
            ),
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
            format_string_like(blocks.div_ceil(2).to_string().as_bytes(), directive.format)
        }
        _ => unreachable!("directive dispatch guarantees metadata directive"),
    })
}

fn render_principal_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
) -> Result<Vec<u8>, Diagnostic> {
    Ok(match directive.kind {
        PrintfDirectiveKind::UserName => {
            let owner = entry.active_owner(follow_mode)?;
            let name = user_name(owner.clone())?;
            format_string_like(
                name_or_id_bytes(name.as_deref(), &owner).as_slice(),
                directive.format,
            )
        }
        PrintfDirectiveKind::UserSid => {
            let owner = entry.active_owner(follow_mode)?;
            format_string_like(principal_id_bytes(&owner).as_slice(), directive.format)
        }
        PrintfDirectiveKind::UserId => format_string_like(
            entry.active_uid(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        PrintfDirectiveKind::GroupName => {
            let group = entry.active_group(follow_mode)?;
            let name = group_name(group.clone())?;
            format_string_like(
                name_or_id_bytes(name.as_deref(), &group).as_slice(),
                directive.format,
            )
        }
        PrintfDirectiveKind::GroupSid => {
            let group = entry.active_group(follow_mode)?;
            format_string_like(principal_id_bytes(&group).as_slice(), directive.format)
        }
        PrintfDirectiveKind::GroupId => format_string_like(
            entry.active_gid(follow_mode)?.to_string().as_bytes(),
            directive.format,
        ),
        _ => unreachable!("directive dispatch guarantees principal directive"),
    })
}

fn render_filesystem_type_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
) -> Result<Vec<u8>, Diagnostic> {
    let snapshot = context.mount_snapshot()?;
    let mount_id = entry.active_mount_id(follow_mode)?;
    let type_name = snapshot.type_for_mount_id(mount_id).ok_or_else(|| {
        Diagnostic::new(
            format!("internal error: mount ID {mount_id} missing from mount snapshot"),
            1,
        )
    })?;
    Ok(format_string_like(
        encoded_bytes(type_name),
        directive.format,
    ))
}

fn render_time_directive(
    directive: &PrintfDirective,
    entry: &EntryContext,
    follow_mode: FollowMode,
    state: &mut PrintfRenderState,
) -> Result<Vec<u8>, Diagnostic> {
    let (family, selector) = match directive.kind {
        PrintfDirectiveKind::FullTimestamp(family) => (family, None),
        PrintfDirectiveKind::TimestampPart { family, selector } => (family, Some(selector)),
        _ => unreachable!("directive dispatch guarantees time directive"),
    };

    let Some(parts) = resolve_cached_time_parts(state, family, entry, follow_mode)? else {
        return Ok(format_string_like(b"", directive.format));
    };

    let bytes = match selector {
        None => render_full_time_bytes(parts)?,
        Some(PrintfTimeSelector::Byte(byte)) => {
            render_selector_bytes(parts, PrintfTimeSelector::Byte(byte))?
        }
        Some(PrintfTimeSelector::EpochSeconds) => {
            render_selector_bytes(parts, PrintfTimeSelector::EpochSeconds)?
        }
        Some(PrintfTimeSelector::GnuPlus) => {
            render_selector_bytes(parts, PrintfTimeSelector::GnuPlus)?
        }
    };
    Ok(format_string_like(&bytes, directive.format))
}
