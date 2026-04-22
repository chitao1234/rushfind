use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
#[cfg(target_os = "openbsd")]
use std::ffi::CString;
use std::ffi::{CStr, OsString};
use std::fs;
use std::io;
#[cfg(target_os = "openbsd")]
use std::mem::MaybeUninit;
#[cfg(target_os = "openbsd")]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Unsupported("reparse type is only supported on Windows"),
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Approximate("interactive locale behavior is approximate on this platform"),
    SupportLevel::Exact,
    SupportLevel::Exact,
);

#[cfg(not(target_os = "macos"))]
pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Unsupported("reparse type is only supported on Windows"),
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Approximate("interactive locale behavior is approximate on this platform"),
    SupportLevel::Exact,
    SupportLevel::Exact,
);

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    true
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    cfg!(target_os = "openbsd")
}

pub(crate) static FLAG_SPECS: &[FlagSpec] = &[
    FlagSpec {
        name: "arch",
        bit: libc::SF_ARCHIVED as u64,
    },
    FlagSpec {
        name: "nodump",
        bit: libc::UF_NODUMP as u64,
    },
    FlagSpec {
        name: "uchg",
        bit: libc::UF_IMMUTABLE as u64,
    },
];

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

#[cfg(any(target_os = "dragonfly", doc))]
const MNT_NOWAIT_FLAG: libc::c_int = 0x0002;
#[cfg(not(any(target_os = "dragonfly", doc)))]
const MNT_NOWAIT_FLAG: libc::c_int = libc::MNT_NOWAIT;

#[cfg(target_os = "netbsd")]
const NETBSD_VFS_NAMELEN: usize = 32;
#[cfg(target_os = "netbsd")]
const NETBSD_VFS_MNAMELEN: usize = 1024;
#[cfg(target_os = "netbsd")]
#[repr(C)]
struct MountEntry {
    f_flag: libc::c_ulong,
    f_bsize: libc::c_ulong,
    f_frsize: libc::c_ulong,
    f_iosize: libc::c_ulong,
    f_blocks: libc::fsblkcnt_t,
    f_bfree: libc::fsblkcnt_t,
    f_bavail: libc::fsblkcnt_t,
    f_bresvd: libc::fsblkcnt_t,
    f_files: libc::fsfilcnt_t,
    f_ffree: libc::fsfilcnt_t,
    f_favail: libc::fsfilcnt_t,
    f_fresvd: libc::fsfilcnt_t,
    f_syncreads: u64,
    f_syncwrites: u64,
    f_asyncreads: u64,
    f_asyncwrites: u64,
    f_fsidx: libc::fsid_t,
    f_fsid: libc::c_ulong,
    f_namemax: libc::c_ulong,
    f_owner: libc::uid_t,
    f_spare: [u64; 4],
    f_fstypename: [libc::c_char; NETBSD_VFS_NAMELEN],
    f_mntonname: [libc::c_char; NETBSD_VFS_MNAMELEN],
    f_mntfromname: [libc::c_char; NETBSD_VFS_MNAMELEN],
    // NetBSD 10 added this trailing field, but libc 0.2.x still exposes the
    // older layout. Using the stale binding makes every later string field read
    // as empty, which breaks -fstype and %F.
    f_mntfromlabel: [libc::c_char; NETBSD_VFS_MNAMELEN],
}
#[cfg(target_os = "netbsd")]
unsafe extern "C" {
    #[link_name = "__getmntinfo90"]
    fn netbsd_getmntinfo(mntbufp: *mut *mut MountEntry, flags: libc::c_int) -> libc::c_int;
}
#[cfg(not(target_os = "netbsd"))]
type MountEntry = libc::statfs;

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    let mounts = load_mount_entries()?;

    let mut snapshot = FilesystemSnapshot::default();
    for mount in mounts {
        let mount_path = mount_target_path(mount);
        let Ok(metadata) = fs::metadata(&mount_path) else {
            continue;
        };
        snapshot.insert(
            FilesystemKey::Numeric(metadata.dev()),
            mount_type_name(mount),
        );
    }
    Ok(snapshot)
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(FilesystemKey::Numeric(metadata.dev()))
}

#[cfg(target_os = "openbsd")]
pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Diagnostic::new(format!("{}: invalid path", path.display()), 1))?;
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    let rc = unsafe {
        if follow {
            libc::stat(c_path.as_ptr(), stat.as_mut_ptr())
        } else {
            libc::lstat(c_path.as_ptr(), stat.as_mut_ptr())
        }
    };
    if rc != 0 {
        return Err(Diagnostic::new(
            format!("{}: {}", path.display(), io::Error::last_os_error()),
            1,
        ));
    }

    let stat = unsafe { stat.assume_init() };
    if stat.st_birthtime == 0 && stat.st_birthtime_nsec == 0 {
        return Ok(None);
    }

    Ok(Some(Timestamp::new(
        stat.st_birthtime,
        stat.st_birthtime_nsec as i32,
    )))
}

pub(crate) fn read_file_flags(path: &Path, follow: bool) -> io::Result<Option<u64>> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(Some(metadata.st_flags() as u64))
}

#[cfg(not(target_os = "openbsd"))]
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

#[cfg(target_os = "netbsd")]
fn load_mount_entries() -> Result<&'static [MountEntry], Diagnostic> {
    let mounts = unsafe {
        let mut mounts: *mut MountEntry = std::ptr::null_mut();
        let count = netbsd_getmntinfo(&mut mounts, MNT_NOWAIT_FLAG);
        if count <= 0 {
            return Err(Diagnostic::new(
                "failed to read mount table via getmntinfo",
                1,
            ));
        }
        std::slice::from_raw_parts(mounts, count as usize)
    };
    Ok(mounts)
}

#[cfg(not(target_os = "netbsd"))]
fn load_mount_entries() -> Result<&'static [MountEntry], Diagnostic> {
    let mounts = unsafe {
        let mut mounts: *mut MountEntry = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut mounts, MNT_NOWAIT_FLAG);
        if count <= 0 {
            return Err(Diagnostic::new(
                "failed to read mount table via getmntinfo",
                1,
            ));
        }
        std::slice::from_raw_parts(mounts, count as usize)
    };
    Ok(mounts)
}
