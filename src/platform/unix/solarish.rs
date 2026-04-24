use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::capabilities::OutputContract;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
use std::ffi::CString;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities {
    fstype: SupportLevel::Exact,
    same_file_system: SupportLevel::Exact,
    birth_time: SupportLevel::Unsupported(
        "birth time predicates are not supported on this platform",
    ),
    file_flags: SupportLevel::Unsupported("`-flags` is not supported on this platform"),
    reparse_type: SupportLevel::Unsupported("reparse type is only supported on Windows"),
    named_ownership: SupportLevel::Exact,
    numeric_ownership: SupportLevel::Exact,
    windows_ownership_predicates: SupportLevel::Unsupported(
        "Windows ownership predicates are only supported on Windows",
    ),
    access_predicates: SupportLevel::Exact,
    messages_locale: SupportLevel::Approximate(
        "interactive locale behavior is approximate on this platform",
    ),
    case_insensitive_glob: SupportLevel::Approximate(
        "case-insensitive glob matching may differ outside the C locale on this platform",
    ),
    mode_bits: SupportLevel::Exact,
    output_contract: OutputContract::Posix,
};

static FLAG_SPECS: &[FlagSpec] = &[];

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &CAPABILITIES
}

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    true
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    false
}

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    let mnttab = fs::read_to_string("/etc/mnttab")
        .map_err(|error| Diagnostic::new(format!("/etc/mnttab: {error}"), 1))?;

    let mut snapshot = FilesystemSnapshot::default();
    for line in mnttab.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let _special = fields.next();
        let Some(mountpoint) = fields.next() else {
            continue;
        };
        let Some(type_name) = fields.next() else {
            continue;
        };

        let Ok(statvfs) = statvfs_for_path(Path::new(mountpoint)) else {
            continue;
        };
        snapshot.insert(
            FilesystemKey::Numeric(statvfs.f_fsid as u64),
            OsString::from(type_name),
        );
    }

    Ok(snapshot)
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let stat_path = statvfs_lookup_path(path, follow)?;
    let statvfs = statvfs_for_path(&stat_path)?;
    Ok(FilesystemKey::Numeric(statvfs.f_fsid as u64))
}

pub(crate) fn read_file_flags(path: &Path, follow: bool) -> io::Result<Option<u64>> {
    let _ = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(None)
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let _ = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;
    Ok(None)
}

fn statvfs_lookup_path(path: &Path, follow: bool) -> io::Result<PathBuf> {
    if follow {
        return Ok(path.to_path_buf());
    }

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        Ok(path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf())
    } else {
        Ok(path.to_path_buf())
    }
}

fn statvfs_for_path(path: &Path) -> io::Result<libc::statvfs> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;
    let mut statvfs = MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), statvfs.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(unsafe { statvfs.assume_init() })
}
