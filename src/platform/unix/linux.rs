use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
use std::ffi::CString;
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const FS_IMMUTABLE_FL: u64 = 0x0000_0010;
const FS_APPEND_FL: u64 = 0x0000_0020;
const FS_NODUMP_FL: u64 = 0x0000_0040;

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Unsupported("reparse type is only supported on Windows"),
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
);

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    false
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    false
}

pub(crate) static FLAG_SPECS: &[FlagSpec] = &[
    FlagSpec {
        name: "append",
        bit: FS_APPEND_FL,
    },
    FlagSpec {
        name: "immutable",
        bit: FS_IMMUTABLE_FL,
    },
    FlagSpec {
        name: "nodump",
        bit: FS_NODUMP_FL,
    },
];

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|error| Diagnostic::new(format!("/proc/self/mountinfo: {error}"), 1))?;
    FilesystemSnapshot::from_mountinfo(&mountinfo)
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;
    let mut statx = MaybeUninit::<libc::statx>::zeroed();
    let flags = if follow {
        libc::AT_STATX_SYNC_AS_STAT
    } else {
        libc::AT_STATX_SYNC_AS_STAT | libc::AT_SYMLINK_NOFOLLOW
    };

    let rc = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            flags,
            libc::STATX_MNT_ID,
            statx.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }

    let statx = unsafe { statx.assume_init() };
    if statx.stx_mask & libc::STATX_MNT_ID == 0 {
        return Err(io::Error::from_raw_os_error(libc::EOPNOTSUPP));
    }

    Ok(FilesystemKey::Numeric(statx.stx_mnt_id))
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Diagnostic::new(format!("{}: invalid path", path.display()), 1))?;
    let mut statx = MaybeUninit::<libc::statx>::zeroed();
    let flags = if follow {
        libc::AT_STATX_SYNC_AS_STAT
    } else {
        libc::AT_STATX_SYNC_AS_STAT | libc::AT_SYMLINK_NOFOLLOW
    };

    let rc = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            flags,
            libc::STATX_BTIME,
            statx.as_mut_ptr(),
        )
    };
    if rc != 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => Ok(None),
            _ => Err(Diagnostic::new(format!("{}: {error}", path.display()), 1)),
        };
    }

    let statx = unsafe { statx.assume_init() };
    if statx.stx_mask & libc::STATX_BTIME == 0 {
        return Ok(None);
    }

    let birth = statx.stx_btime;
    if birth.tv_sec == 0 && birth.tv_nsec == 0 {
        return Ok(None);
    }

    Ok(Some(Timestamp::new(birth.tv_sec, birth.tv_nsec as i32)))
}

pub(crate) fn read_file_flags(path: &Path, follow: bool) -> io::Result<Option<u64>> {
    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_CLOEXEC);
    if !follow {
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error)
            if !follow
                && matches!(
                    error.raw_os_error(),
                    Some(libc::ELOOP) | Some(libc::EMLINK) | Some(libc::ENOENT)
                ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    let mut bits: libc::c_long = 0;
    let rc = unsafe {
        libc::ioctl(
            std::os::fd::AsRawFd::as_raw_fd(&file),
            libc::FS_IOC_GETFLAGS,
            &mut bits,
        )
    };
    if rc != 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOTTY) | Some(libc::EOPNOTSUPP) | Some(libc::ENOSYS) => Ok(None),
            _ => Err(error),
        };
    }

    Ok(Some(bits as u64))
}
