use super::PlatformCapabilities;
use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::time::Timestamp;
use std::io;
use std::path::Path;

#[cfg(any(
    test,
    not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))
))]
pub(crate) mod generic;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
use self::linux as backend;

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
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
use self::generic as backend;

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    backend::active_capabilities()
}

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    backend::active_flag_specs()
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

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    backend::filesystem_key(path, follow)
}

pub(crate) fn read_file_flags(path: &Path, follow: bool) -> io::Result<Option<u64>> {
    backend::read_file_flags(path, follow)
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    backend::read_birth_time(path, follow)
}
