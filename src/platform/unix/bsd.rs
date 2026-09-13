use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::capabilities::OutputContract;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot, MetadataExtras};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
#[cfg(target_os = "openbsd")]
use std::ffi::CString;
use std::ffi::{CStr, OsString};
use std::fs;
use std::io;
#[cfg(target_os = "openbsd")]
use std::mem::MaybeUninit;
#[cfg(target_os = "dragonfly")]
use std::os::dragonfly::fs::MetadataExt as BsdFlagsMetadataExt;
#[cfg(target_os = "freebsd")]
use std::os::freebsd::fs::MetadataExt as BsdFlagsMetadataExt;
#[cfg(target_os = "macos")]
use std::os::macos::fs::MetadataExt as BsdFlagsMetadataExt;
#[cfg(target_os = "netbsd")]
use std::os::netbsd::fs::MetadataExt as BsdFlagsMetadataExt;
#[cfg(target_os = "openbsd")]
use std::os::openbsd::fs::MetadataExt as BsdFlagsMetadataExt;
#[cfg(target_os = "openbsd")]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities {
    fstype: SupportLevel::Exact,
    same_file_system: SupportLevel::Exact,
    birth_time: SupportLevel::Exact,
    file_flags: SupportLevel::Exact,
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
    case_insensitive_glob: SupportLevel::Exact,
    mode_bits: SupportLevel::Exact,
    output_contract: OutputContract::Posix,
};

#[cfg(not(target_os = "macos"))]
pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities {
    fstype: SupportLevel::Exact,
    same_file_system: SupportLevel::Exact,
    birth_time: SupportLevel::Exact,
    file_flags: SupportLevel::Exact,
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
    case_insensitive_glob: SupportLevel::Exact,
    mode_bits: SupportLevel::Exact,
    output_contract: OutputContract::Posix,
};

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &CAPABILITIES
}

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
        name: "archived",
        bit: libc::SF_ARCHIVED as u64,
    },
    FlagSpec {
        name: "nodump",
        bit: libc::UF_NODUMP as u64,
    },
    FlagSpec {
        name: "opaque",
        bit: libc::UF_OPAQUE as u64,
    },
    FlagSpec {
        name: "sappnd",
        bit: libc::SF_APPEND as u64,
    },
    FlagSpec {
        name: "sappend",
        bit: libc::SF_APPEND as u64,
    },
    FlagSpec {
        name: "schg",
        bit: libc::SF_IMMUTABLE as u64,
    },
    FlagSpec {
        name: "schange",
        bit: libc::SF_IMMUTABLE as u64,
    },
    FlagSpec {
        name: "simmutable",
        bit: libc::SF_IMMUTABLE as u64,
    },
    FlagSpec {
        name: "uappnd",
        bit: libc::UF_APPEND as u64,
    },
    FlagSpec {
        name: "uappend",
        bit: libc::UF_APPEND as u64,
    },
    FlagSpec {
        name: "uchg",
        bit: libc::UF_IMMUTABLE as u64,
    },
    FlagSpec {
        name: "uchange",
        bit: libc::UF_IMMUTABLE as u64,
    },
    FlagSpec {
        name: "uimmutable",
        bit: libc::UF_IMMUTABLE as u64,
    },
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    FlagSpec {
        name: "hidden",
        bit: libc::UF_HIDDEN as u64,
    },
    #[cfg(target_os = "macos")]
    FlagSpec {
        name: "compressed",
        bit: libc::UF_COMPRESSED as u64,
    },
    #[cfg(target_os = "macos")]
    FlagSpec {
        name: "datavault",
        bit: 0x0000_0080,
    },
    #[cfg(target_os = "macos")]
    FlagSpec {
        name: "restricted",
        bit: 0x0008_0000,
    },
    #[cfg(target_os = "macos")]
    FlagSpec {
        name: "sunlnk",
        bit: 0x0010_0000,
    },
    #[cfg(target_os = "macos")]
    FlagSpec {
        name: "sunlink",
        bit: 0x0010_0000,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "offline",
        bit: libc::UF_OFFLINE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "readonly",
        bit: libc::UF_READONLY as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "sparse",
        bit: libc::UF_SPARSE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "system",
        bit: libc::UF_SYSTEM as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "reparse",
        bit: libc::UF_REPARSE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uarch",
        bit: libc::UF_ARCHIVE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uarchive",
        bit: libc::UF_ARCHIVE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uhidden",
        bit: libc::UF_HIDDEN as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uoffline",
        bit: libc::UF_OFFLINE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "urdonly",
        bit: libc::UF_READONLY as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "usparse",
        bit: libc::UF_SPARSE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "usystem",
        bit: libc::UF_SYSTEM as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "ureparse",
        bit: libc::UF_REPARSE as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uunlnk",
        bit: libc::UF_NOUNLINK as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "uunlink",
        bit: libc::UF_NOUNLINK as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "sunlnk",
        bit: libc::SF_NOUNLINK as u64,
    },
    #[cfg(target_os = "freebsd")]
    FlagSpec {
        name: "sunlink",
        bit: libc::SF_NOUNLINK as u64,
    },
];

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

pub(crate) fn active_flag_mask() -> u64 {
    let mut mask = FLAG_SPECS.iter().fold(0u64, |mask, spec| mask | spec.bit);
    #[cfg(target_os = "macos")]
    {
        mask |= libc::UF_TRACKED as u64;
        mask |= libc::UF_COMPRESSED as u64;
        mask |= 0x0000_0080; // UF_DATAVAULT
        mask |= 0x0008_0000; // SF_RESTRICTED
        mask |= 0x0080_0000; // SF_FIRMLINK
        mask |= 0x0010_0000; // SF_NOUNLINK
    }
    mask
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

/// Flags, birth time and device, all read from the `lstat` the caller already
/// performed: BSD `stat` carries them, so this costs no further syscall.
pub(crate) fn metadata_extras(
    _path: &Path,
    metadata: &fs::Metadata,
    _follow: bool,
) -> MetadataExtras {
    MetadataExtras {
        flag_bits: Some(metadata.st_flags() as u64),
        birth_time: birth_time(metadata),
        filesystem_key: Some(FilesystemKey::Numeric(metadata.dev())),
    }
}

#[cfg(target_os = "openbsd")]
fn birth_time(metadata: &fs::Metadata) -> Option<Timestamp> {
    let seconds = metadata.st_birthtime();
    let nanos = metadata.st_birthtime_nsec();
    // OpenBSD reports "no birth time" as zero instead of omitting the field.
    if seconds == 0 && nanos == 0 {
        return None;
    }

    Some(Timestamp::new(seconds, nanos as i32))
}

#[cfg(not(target_os = "openbsd"))]
fn birth_time(metadata: &fs::Metadata) -> Option<Timestamp> {
    metadata
        .created()
        .ok()
        .and_then(|time| Timestamp::from_system_time(time).ok())
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
