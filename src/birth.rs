use crate::diagnostics::Diagnostic;
use crate::time::Timestamp;
use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
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

    Ok(Some(Timestamp::new(
        birth.tv_sec as i64,
        birth.tv_nsec as i32,
    )))
}
