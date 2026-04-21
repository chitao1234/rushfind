use super::PlatformCapabilities;
use crate::diagnostics::Diagnostic;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::time::Timestamp;
use std::io;
use std::path::Path;

#[cfg(any(target_os = "linux", doc))]
pub(crate) mod linux;

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    doc
))]
pub(crate) mod bsd;

#[cfg(target_os = "linux")]
pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &linux::CAPABILITIES
}

#[cfg(target_os = "linux")]
pub(crate) fn printf_zero_pads_string_fields() -> bool {
    linux::printf_zero_pads_string_fields()
}

#[cfg(target_os = "linux")]
pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    linux::used_requires_strict_atime_after_ctime()
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &bsd::CAPABILITIES
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn printf_zero_pads_string_fields() -> bool {
    bsd::printf_zero_pads_string_fields()
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    bsd::used_requires_strict_atime_after_ctime()
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    panic!("unix-family phase 1 only supports Linux, macOS, and BSD backends")
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
pub(crate) fn printf_zero_pads_string_fields() -> bool {
    panic!("unix-family phase 1 only supports Linux, macOS, and BSD backends")
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    panic!("unix-family phase 1 only supports Linux, macOS, and BSD backends")
}

#[cfg(target_os = "linux")]
pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    linux::filesystem_snapshot()
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    bsd::filesystem_snapshot()
}

#[cfg(target_os = "linux")]
pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    linux::filesystem_key(path, follow)
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    bsd::filesystem_key(path, follow)
}

#[cfg(target_os = "linux")]
pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    linux::read_birth_time(path, follow)
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    bsd::read_birth_time(path, follow)
}
