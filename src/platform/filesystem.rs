use crate::diagnostics::Diagnostic;
use crate::entry::{AccessMode, EntryKind, file_type_to_kind};
use crate::identity::FileIdentity;
use crate::time::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FilesystemKey(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformMetadataView {
    pub kind: EntryKind,
    pub identity: Option<FileIdentity>,
    pub size: u64,
    pub owner: Option<u32>,
    pub group: Option<u32>,
    pub mode_bits: Option<u32>,
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
    pub(crate) fn load_proc_self_mountinfo() -> Result<Self, Diagnostic> {
        crate::platform::unix::filesystem_snapshot()
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

            snapshot.insert(FilesystemKey(mount_id), OsString::from(file_system_type));
        }

        Ok(snapshot)
    }

    pub(crate) fn knows_type(&self, type_name: &OsStr) -> bool {
        self.known_types.contains(type_name)
    }

    pub(crate) fn type_for_mount_id(&self, mount_id: u64) -> Option<&OsStr> {
        self.types_by_key
            .get(&FilesystemKey(mount_id))
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

pub(crate) fn load_metadata_view(path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(metadata_view_from_metadata(path, &metadata, follow))
}

pub(crate) fn metadata_view_from_metadata(
    path: &Path,
    metadata: &Metadata,
    follow: bool,
) -> PlatformMetadataView {
    let kind = file_type_to_kind(metadata.file_type());

    PlatformMetadataView {
        kind,
        identity: Some(FileIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }),
        size: metadata.len(),
        owner: Some(metadata.uid()),
        group: Some(metadata.gid()),
        mode_bits: Some(metadata.mode() & 0o7777),
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

pub(crate) fn missing_field(label: &str, path: &Path) -> Diagnostic {
    Diagnostic::new(
        format!(
            "{}: {label} is not available on this platform",
            path.display()
        ),
        1,
    )
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    crate::platform::unix::filesystem_key(path, follow)
}

pub(crate) fn read_access(path: &Path, mode: AccessMode) -> io::Result<bool> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?;

    read_access_with(c_path.as_ptr(), mode, faccessat_access, access_access)
}

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

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    crate::platform::unix::read_birth_time(path, follow)
}

fn faccessat_access(path: *const libc::c_char, mode: AccessMode) -> io::Result<bool> {
    let rc = unsafe { libc::faccessat(libc::AT_FDCWD, path, mode.as_flag(), 0) };
    if rc == 0 {
        Ok(true)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn access_access(path: *const libc::c_char, mode: AccessMode) -> io::Result<bool> {
    let rc = unsafe { libc::access(path, mode.as_flag()) };
    if rc == 0 { Ok(true) } else { Ok(false) }
}
