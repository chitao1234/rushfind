use crate::diagnostics::Diagnostic;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
use std::ffi::CString;
use std::fs;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
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

    Ok(FilesystemKey(statx.stx_mnt_id))
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
