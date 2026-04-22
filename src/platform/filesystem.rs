#![cfg_attr(windows, allow(dead_code))]

use crate::diagnostics::Diagnostic;
use crate::entry::{AccessMode, EntryKind, file_type_to_kind};
use crate::identity::FileIdentity;
use crate::time::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::ffi::CString;
use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FilesystemKey {
    Numeric(u64),
    Text(OsString),
}

impl FilesystemKey {
    pub(crate) fn numeric(&self) -> Option<u64> {
        match self {
            Self::Numeric(value) => Some(*value),
            Self::Text(_) => None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlatformPrincipalId {
    Numeric(u32),
    Sid(String),
}

impl PlatformPrincipalId {
    pub(crate) fn numeric(&self) -> Option<u32> {
        match self {
            Self::Numeric(value) => Some(*value),
            Self::Sid(_) => None,
        }
    }
}

impl From<u32> for PlatformPrincipalId {
    fn from(value: u32) -> Self {
        Self::Numeric(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformMetadataView {
    pub kind: EntryKind,
    pub identity: Option<FileIdentity>,
    pub size: u64,
    pub owner: Option<PlatformPrincipalId>,
    pub group: Option<PlatformPrincipalId>,
    pub mode_bits: Option<u32>,
    pub native_attributes: Option<u32>,
    pub reparse_tag: Option<u32>,
    pub link_count: Option<u64>,
    pub blocks_512: Option<u64>,
    pub atime: Timestamp,
    pub ctime: Timestamp,
    pub mtime: Timestamp,
    pub birth_time: Option<Timestamp>,
    pub filesystem_key: Option<FilesystemKey>,
    pub device_number: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FilesystemSnapshot {
    types_by_key: BTreeMap<FilesystemKey, OsString>,
    known_types: BTreeSet<OsString>,
}

pub(crate) trait PlatformReader: Send + Sync {
    fn metadata_view(&self, path: &Path, follow: bool) -> io::Result<PlatformMetadataView>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn directory_is_empty(&self, path: &Path) -> io::Result<bool>;
    fn access(&self, path: &Path, mode: AccessMode) -> io::Result<bool>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FsPlatformReader;

impl PlatformReader for FsPlatformReader {
    fn metadata_view(&self, path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
        load_metadata_view(path, follow)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }

    fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
        let mut entries = fs::read_dir(path)?;
        match entries.next() {
            None => Ok(true),
            Some(result) => result.map(|_| false),
        }
    }

    fn access(&self, path: &Path, mode: AccessMode) -> io::Result<bool> {
        read_access(path, mode)
    }
}

impl FilesystemSnapshot {
    #[cfg(unix)]
    pub(crate) fn load_proc_self_mountinfo() -> Result<Self, Diagnostic> {
        crate::platform::unix::filesystem_snapshot()
    }

    #[cfg(windows)]
    pub(crate) fn load_proc_self_mountinfo() -> Result<Self, Diagnostic> {
        crate::platform::windows::filesystem::filesystem_snapshot()
    }

    #[cfg(any(test, target_os = "linux"))]
    pub(crate) fn from_mountinfo(mountinfo: &str) -> Result<Self, Diagnostic> {
        let mut snapshot = Self::default();

        for line in mountinfo.lines().filter(|line| !line.trim().is_empty()) {
            let (left, right) = line
                .split_once(" - ")
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?;
            let mount_id = left
                .split_whitespace()
                .next()
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?
                .parse::<u64>()
                .map_err(|_| Diagnostic::new(format!("invalid mount ID in `{line}`"), 1))?;
            let file_system_type = right
                .split_whitespace()
                .next()
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?;

            snapshot.insert(
                FilesystemKey::Numeric(mount_id),
                OsString::from(file_system_type),
            );
        }

        Ok(snapshot)
    }

    pub(crate) fn knows_type(&self, type_name: &OsStr) -> bool {
        self.known_types.contains(type_name)
    }

    pub(crate) fn type_for_mount_id(&self, mount_id: u64) -> Option<&OsStr> {
        self.types_by_key
            .get(&FilesystemKey::Numeric(mount_id))
            .map(|type_name| type_name.as_os_str())
    }

    pub(crate) fn type_for_mount_key(&self, key: FilesystemKey) -> Option<&OsStr> {
        self.types_by_key
            .get(&key)
            .map(|type_name| type_name.as_os_str())
    }

    pub(crate) fn insert(&mut self, key: FilesystemKey, type_name: OsString) {
        self.known_types.insert(type_name.clone());
        self.types_by_key.insert(key, type_name);
    }
}

#[cfg(unix)]
pub(crate) fn load_metadata_view(path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(metadata_view_from_metadata(path, &metadata, follow))
}

#[cfg(windows)]
pub(crate) fn load_metadata_view(path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
    crate::platform::windows::filesystem::metadata_view(path, follow)
}

#[cfg(unix)]
pub(crate) fn metadata_view_from_metadata(
    path: &Path,
    metadata: &Metadata,
    follow: bool,
) -> PlatformMetadataView {
    use std::os::unix::fs::MetadataExt;

    let kind = file_type_to_kind(metadata.file_type());

    PlatformMetadataView {
        kind,
        identity: Some(FileIdentity::from_metadata(metadata)),
        size: metadata.len(),
        owner: Some(PlatformPrincipalId::Numeric(metadata.uid())),
        group: Some(PlatformPrincipalId::Numeric(metadata.gid())),
        mode_bits: Some(metadata.mode() & 0o7777),
        native_attributes: None,
        reparse_tag: None,
        link_count: Some(metadata.nlink()),
        blocks_512: Some(metadata.blocks()),
        atime: Timestamp::new(metadata.atime(), metadata.atime_nsec() as i32),
        ctime: Timestamp::new(metadata.ctime(), metadata.ctime_nsec() as i32),
        mtime: Timestamp::new(metadata.mtime(), metadata.mtime_nsec() as i32),
        birth_time: read_birth_time(path, follow).ok().flatten(),
        filesystem_key: filesystem_key(path, follow).ok(),
        device_number: match kind {
            EntryKind::Block | EntryKind::Character => Some(metadata.rdev()),
            _ => None,
        },
    }
}

#[cfg(windows)]
pub(crate) fn metadata_view_from_metadata(
    _path: &Path,
    metadata: &Metadata,
    _follow: bool,
) -> PlatformMetadataView {
    PlatformMetadataView {
        kind: file_type_to_kind(metadata.file_type()),
        identity: None,
        size: metadata.len(),
        owner: None,
        group: None,
        mode_bits: None,
        native_attributes: None,
        reparse_tag: None,
        link_count: None,
        blocks_512: None,
        atime: Timestamp::new(0, 0),
        ctime: Timestamp::new(0, 0),
        mtime: Timestamp::new(0, 0),
        birth_time: None,
        filesystem_key: None,
        device_number: None,
    }
}

pub(crate) fn missing_field(label: &str, path: &Path) -> Diagnostic {
    Diagnostic::new(
        format!(
            "{}: {label} is not available on this platform",
            path.display()
        ),
        1,
    )
}

#[cfg(unix)]
pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    crate::platform::unix::filesystem_key(path, follow)
}

#[cfg(windows)]
pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    crate::platform::windows::filesystem::filesystem_key(path, follow)
}

#[cfg(unix)]
pub(crate) fn read_access(path: &Path, mode: AccessMode) -> io::Result<bool> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;

    read_access_with(c_path.as_ptr(), mode, faccessat_access, access_access)
}

#[cfg(windows)]
pub(crate) fn read_access(path: &Path, mode: AccessMode) -> io::Result<bool> {
    crate::platform::windows::filesystem::read_access(path, mode)
}

#[cfg(unix)]
pub(crate) fn read_access_with(
    path: *const libc::c_char,
    mode: AccessMode,
    primary: impl FnOnce(*const libc::c_char, AccessMode) -> io::Result<bool>,
    fallback: impl FnOnce(*const libc::c_char, AccessMode) -> io::Result<bool>,
) -> io::Result<bool> {
    match primary(path, mode) {
        Ok(result) => Ok(result),
        Err(error) if error.raw_os_error() == Some(libc::ENOSYS) => fallback(path, mode),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    crate::platform::unix::read_birth_time(path, follow)
}

#[cfg(windows)]
pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    crate::platform::windows::filesystem::read_birth_time(path, follow)
}

pub(crate) fn is_traversal_link(view: &PlatformMetadataView) -> bool {
    if view.kind == EntryKind::Symlink {
        return true;
    }

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
    view.reparse_tag == Some(IO_REPARSE_TAG_MOUNT_POINT)
}

#[cfg(unix)]
fn faccessat_access(path: *const libc::c_char, mode: AccessMode) -> io::Result<bool> {
    let rc = unsafe { libc::faccessat(libc::AT_FDCWD, path, mode.as_flag(), 0) };
    if rc == 0 {
        Ok(true)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn access_access(path: *const libc::c_char, mode: AccessMode) -> io::Result<bool> {
    let rc = unsafe { libc::access(path, mode.as_flag()) };
    if rc == 0 { Ok(true) } else { Ok(false) }
}

#[allow(dead_code)]
fn unsupported_io(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}
