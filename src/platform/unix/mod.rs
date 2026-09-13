use super::PlatformCapabilities;
use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::filesystem::{FilesystemSnapshot, MetadataExtras};
use crate::time::Timestamp;
use std::fs::Metadata;
use std::path::Path;

#[cfg(any(
    test,
    not(any(
        target_os = "linux",
        target_os = "illumos",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "solaris"
    ))
))]
pub(crate) mod generic;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
use self::linux as backend;

#[cfg(any(target_os = "illumos", target_os = "solaris"))]
pub(crate) mod solarish;
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
use self::solarish as backend;

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) mod bsd;
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
use self::bsd as backend;

#[cfg(not(any(
    target_os = "linux",
    target_os = "illumos",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "solaris"
)))]
use self::generic as backend;

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    backend::active_capabilities()
}

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    backend::active_flag_specs()
}

pub(crate) fn active_flag_mask() -> u64 {
    backend::active_flag_mask()
}

pub(crate) fn printf_zero_pads_string_fields() -> bool {
    backend::printf_zero_pads_string_fields()
}

pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    backend::used_requires_strict_atime_after_ctime()
}

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    backend::filesystem_snapshot()
}

/// Fields a `PlatformMetadataView` cannot take from the `stat` its caller
/// already performed, answered from the metadata where the platform allows it.
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn metadata_extras(path: &Path, metadata: &Metadata, follow: bool) -> MetadataExtras {
    backend::metadata_extras(path, metadata, follow)
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
pub(crate) fn metadata_extras(path: &Path, _metadata: &Metadata, follow: bool) -> MetadataExtras {
    // The Linux flags live behind `FS_IOC_GETFLAGS`, and its birth time and
    // mount ID behind `statx`, so none of them come with `Metadata` for free.
    MetadataExtras {
        flag_bits: backend::read_file_flags(path, follow).ok().flatten(),
        birth_time: backend::read_birth_time(path, follow).ok().flatten(),
        filesystem_key: backend::filesystem_key(path, follow).ok(),
    }
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    backend::read_birth_time(path, follow)
}
