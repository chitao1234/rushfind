use crate::diagnostics::Diagnostic;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
use std::ffi::{CStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Approximate("interactive locale behavior is approximate on this platform"),
    SupportLevel::Exact,
);

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    true
}

#[cfg(target_os = "netbsd")]
type MountEntry = libc::statvfs;
#[cfg(not(target_os = "netbsd"))]
type MountEntry = libc::statfs;

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    let mounts = unsafe {
        let mut mounts: *mut MountEntry = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT);
        if count <= 0 {
            return Err(Diagnostic::new(
                "failed to read mount table via getmntinfo",
                1,
            ));
        }
        std::slice::from_raw_parts(mounts, count as usize)
    };

    let mut snapshot = FilesystemSnapshot::default();
    for mount in mounts {
        let mount_path = mount_target_path(mount);
        let Ok(metadata) = fs::metadata(&mount_path) else {
            continue;
        };
        snapshot.insert(FilesystemKey(metadata.dev()), mount_type_name(mount));
    }
    Ok(snapshot)
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(FilesystemKey(metadata.dev()))
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;

    match metadata.created() {
        Ok(time) => Ok(Some(Timestamp::from_system_time(time)?)),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => Ok(None),
        Err(_) => Ok(None),
    }
}

fn mount_type_name(mount: &MountEntry) -> OsString {
    unsafe {
        OsString::from_vec(
            CStr::from_ptr(mount.f_fstypename.as_ptr())
                .to_bytes()
                .to_vec(),
        )
    }
}

fn mount_target_path(mount: &MountEntry) -> PathBuf {
    let bytes = unsafe {
        CStr::from_ptr(mount.f_mntonname.as_ptr())
            .to_bytes()
            .to_vec()
    };
    PathBuf::from(OsString::from_vec(bytes))
}
