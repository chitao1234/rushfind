use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::platform::filesystem::{
    FsPlatformReader, PlatformMetadataView, PlatformReader, missing_field,
};
use crate::time::Timestamp;
use std::ffi::OsString;
use std::fmt;
use std::fs::FileType;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Block,
    Character,
    Fifo,
    Socket,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintfTargetKind {
    Kind(EntryKind),
    Loop,
    Missing,
    OtherError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Read,
    Write,
    Execute,
}

impl AccessMode {
    pub(crate) fn as_flag(self) -> libc::c_int {
        match self {
            Self::Read => libc::R_OK,
            Self::Write => libc::W_OK,
            Self::Execute => libc::X_OK,
        }
    }
}

#[derive(Clone)]
pub struct EntryContext {
    pub path: PathBuf,
    pub depth: usize,
    pub is_command_line_root: bool,
    root_path: Arc<PathBuf>,
    data: Arc<EntryData>,
}

struct EntryData {
    reader: Arc<dyn PlatformReader>,
    physical_file_type_hint: Option<FileType>,
    physical_view: OnceLock<Result<PlatformMetadataView, Diagnostic>>,
    logical_view: OnceLock<Option<PlatformMetadataView>>,
    physical_link_target: OnceLock<Result<Option<OsString>, Diagnostic>>,
    active_directory_empty: OnceLock<Result<bool, Diagnostic>>,
    readable: OnceLock<bool>,
    writable: OnceLock<bool>,
    executable: OnceLock<bool>,
}

impl fmt::Debug for EntryContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryContext")
            .field("path", &self.path)
            .field("depth", &self.depth)
            .field("is_command_line_root", &self.is_command_line_root)
            .finish_non_exhaustive()
    }
}

impl EntryContext {
    pub fn new(path: PathBuf, depth: usize, is_command_line_root: bool) -> Self {
        let root_path = Arc::new(path.clone());
        Self::with_reader_hint_and_root(
            path,
            depth,
            is_command_line_root,
            root_path,
            None,
            Arc::new(FsPlatformReader),
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn with_file_type_hint(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        physical_file_type_hint: Option<FileType>,
    ) -> Self {
        let root_path = Arc::new(path.clone());
        Self::with_reader_hint_and_root(
            path,
            depth,
            is_command_line_root,
            root_path,
            physical_file_type_hint,
            Arc::new(FsPlatformReader),
        )
    }

    pub(crate) fn with_file_type_hint_and_root(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        root_path: Arc<PathBuf>,
        physical_file_type_hint: Option<FileType>,
    ) -> Self {
        Self::with_reader_hint_and_root(
            path,
            depth,
            is_command_line_root,
            root_path,
            physical_file_type_hint,
            Arc::new(FsPlatformReader),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_reader(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        reader: Arc<dyn PlatformReader>,
    ) -> Self {
        let root_path = Arc::new(path.clone());
        Self::with_reader_hint_and_root(path, depth, is_command_line_root, root_path, None, reader)
    }

    #[cfg(test)]
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn new_with_reader_and_root(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        root_path: Arc<PathBuf>,
        reader: Arc<dyn PlatformReader>,
    ) -> Self {
        Self::with_reader_hint_and_root(path, depth, is_command_line_root, root_path, None, reader)
    }

    fn with_reader_hint_and_root(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        root_path: Arc<PathBuf>,
        physical_file_type_hint: Option<FileType>,
        reader: Arc<dyn PlatformReader>,
    ) -> Self {
        Self {
            path,
            depth,
            is_command_line_root,
            root_path,
            data: Arc::new(EntryData {
                reader,
                physical_file_type_hint,
                physical_view: OnceLock::new(),
                logical_view: OnceLock::new(),
                physical_link_target: OnceLock::new(),
                active_directory_empty: OnceLock::new(),
                readable: OnceLock::new(),
                writable: OnceLock::new(),
                executable: OnceLock::new(),
            }),
        }
    }

    pub fn relative_to_root(&self) -> Result<&Path, Diagnostic> {
        self.path
            .strip_prefix(self.root_path.as_path())
            .map_err(|_| Diagnostic::new("internal error: entry root mismatch", 1))
    }

    pub fn dirname_for_printf(&self) -> PathBuf {
        if !self.path.as_os_str().as_encoded_bytes().contains(&b'/') {
            return PathBuf::from(".");
        }

        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }

    pub fn start_path(&self) -> &Path {
        self.root_path.as_path()
    }

    pub fn physical_kind(&self) -> Result<EntryKind, Diagnostic> {
        if let Some(file_type) = self.data.physical_file_type_hint {
            return Ok(file_type_to_kind(file_type));
        }

        Ok(self.physical_view()?.kind)
    }

    pub fn physical_identity(&self) -> Result<FileIdentity, Diagnostic> {
        self.physical_view()?
            .identity
            .ok_or_else(|| missing_field("file identity", &self.path))
    }

    fn active_view(&self, follow_mode: FollowMode) -> Result<&PlatformMetadataView, Diagnostic> {
        if self.uses_logical_view(follow_mode)
            && self.physical_kind()? == EntryKind::Symlink
            && let Some(view) = self.logical_view()
        {
            return Ok(view);
        }

        self.physical_view()
    }

    pub fn logical_kind(&self) -> Result<EntryKind, Diagnostic> {
        if self.physical_kind()? != EntryKind::Symlink {
            return self.physical_kind();
        }

        if let Some(view) = self.logical_view() {
            Ok(view.kind)
        } else {
            self.physical_kind()
        }
    }

    pub fn logical_identity(&self) -> Option<FileIdentity> {
        if self.physical_kind().ok()? != EntryKind::Symlink {
            return self.physical_identity().ok();
        }

        self.logical_view().and_then(|view| view.identity)
    }

    pub fn active_identity(&self, follow_mode: FollowMode) -> Result<FileIdentity, Diagnostic> {
        if self.uses_logical_view(follow_mode)
            && self.physical_kind()? == EntryKind::Symlink
            && let Some(identity) = self.logical_identity()
        {
            return Ok(identity);
        }

        self.physical_identity()
    }

    pub fn active_inode(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_identity(follow_mode)?.ino)
    }

    pub fn active_device(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_identity(follow_mode)?.dev)
    }

    pub fn active_device_number(&self, follow_mode: FollowMode) -> Result<Option<u64>, Diagnostic> {
        Ok(self.active_view(follow_mode)?.device_number)
    }

    pub fn active_uid(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        self.active_view(follow_mode)?
            .owner
            .ok_or_else(|| missing_field("owner id", &self.path))
    }

    pub fn active_gid(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        self.active_view(follow_mode)?
            .group
            .ok_or_else(|| missing_field("group id", &self.path))
    }

    pub fn active_mode_bits(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        self.active_view(follow_mode)?
            .mode_bits
            .ok_or_else(|| missing_field("mode bits", &self.path))
    }

    pub fn active_size(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_view(follow_mode)?.size)
    }

    pub fn active_atime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        Ok(self.active_view(follow_mode)?.atime)
    }

    pub fn active_ctime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        Ok(self.active_view(follow_mode)?.ctime)
    }

    pub fn active_mtime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        Ok(self.active_view(follow_mode)?.mtime)
    }

    pub fn active_link_count(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        self.active_view(follow_mode)?
            .link_count
            .ok_or_else(|| missing_field("link count", &self.path))
    }

    pub fn active_blocks(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        self.active_view(follow_mode)?
            .blocks_512
            .ok_or_else(|| missing_field("block count", &self.path))
    }

    pub fn access(&self, mode: AccessMode) -> Result<bool, Diagnostic> {
        let cache = match mode {
            AccessMode::Read => &self.data.readable,
            AccessMode::Write => &self.data.writable,
            AccessMode::Execute => &self.data.executable,
        };

        Ok(*cache.get_or_init(|| self.data.reader.access(&self.path, mode).unwrap_or(false)))
    }

    pub fn active_mount_id(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self
            .active_view(follow_mode)?
            .filesystem_key
            .ok_or_else(|| missing_field("filesystem boundary key", &self.path))?
            .0)
    }

    pub fn active_birth_time(
        &self,
        follow_mode: FollowMode,
    ) -> Result<Option<Timestamp>, Diagnostic> {
        Ok(self.active_view(follow_mode)?.birth_time)
    }

    pub fn active_is_empty(&self, follow_mode: FollowMode) -> Result<bool, Diagnostic> {
        match self.active_kind(follow_mode)? {
            EntryKind::File => Ok(self.active_size(follow_mode)? == 0),
            EntryKind::Directory => {
                // GNU find evaluates predicates against one metadata snapshot per entry.
                // Loading the active metadata before probing the directory prevents the
                // probe from changing atime and influencing later metadata predicates.
                let _ = self.active_view(follow_mode)?;

                match self.data.active_directory_empty.get_or_init(|| {
                    self.data
                        .reader
                        .directory_is_empty(&self.path)
                        .map_err(|error| path_error(&self.path, error))
                }) {
                    Ok(is_empty) => Ok(*is_empty),
                    Err(error) => Err(error.clone()),
                }
            }
            _ => Ok(false),
        }
    }

    pub fn active_kind(&self, follow_mode: FollowMode) -> Result<EntryKind, Diagnostic> {
        if self.uses_logical_view(follow_mode)
            && self.physical_kind()? == EntryKind::Symlink
            && let Some(view) = self.logical_view()
        {
            return Ok(view.kind);
        }

        self.physical_kind()
    }

    pub fn printf_target_kind(
        &self,
        follow_mode: FollowMode,
    ) -> Result<PrintfTargetKind, Diagnostic> {
        let active_kind = self.active_kind(follow_mode)?;
        if active_kind != EntryKind::Symlink {
            return Ok(PrintfTargetKind::Kind(active_kind));
        }

        match self.data.reader.metadata_view(&self.path, true) {
            Ok(view) => Ok(PrintfTargetKind::Kind(view.kind)),
            Err(error) => Ok(match error.raw_os_error() {
                Some(libc::ELOOP) => PrintfTargetKind::Loop,
                Some(libc::ENOENT) => PrintfTargetKind::Missing,
                _ => PrintfTargetKind::OtherError,
            }),
        }
    }

    pub fn active_directory_identity(
        &self,
        follow_mode: FollowMode,
    ) -> Result<Option<FileIdentity>, Diagnostic> {
        if self.active_kind(follow_mode)? != EntryKind::Directory {
            return Ok(None);
        }

        Ok(Some(self.active_identity(follow_mode)?))
    }

    pub fn xtype_kind(&self, follow_mode: FollowMode) -> Result<EntryKind, Diagnostic> {
        match follow_mode {
            FollowMode::Logical => self.physical_kind(),
            FollowMode::Physical | FollowMode::CommandLineOnly => self.logical_kind(),
        }
    }

    pub fn physical_link_target(&self) -> Result<Option<OsString>, Diagnostic> {
        match self.data.physical_link_target.get_or_init(|| {
            if self.physical_kind()? != EntryKind::Symlink {
                return Ok(None);
            }

            self.data
                .reader
                .read_link(&self.path)
                .map(|target| Some(target.into_os_string()))
                .map_err(|error| path_error(&self.path, error))
        }) {
            Ok(target) => Ok(target.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    pub fn active_link_target(
        &self,
        follow_mode: FollowMode,
    ) -> Result<Option<OsString>, Diagnostic> {
        if self.physical_kind()? != EntryKind::Symlink {
            return Ok(None);
        }

        match follow_mode {
            FollowMode::Physical => self.physical_link_target(),
            FollowMode::CommandLineOnly if self.is_command_line_root => {
                if self.logical_view().is_some() {
                    Ok(None)
                } else {
                    self.physical_link_target()
                }
            }
            FollowMode::CommandLineOnly => self.physical_link_target(),
            FollowMode::Logical => {
                if self.logical_view().is_some() {
                    Ok(None)
                } else {
                    self.physical_link_target()
                }
            }
        }
    }

    fn uses_logical_view(&self, follow_mode: FollowMode) -> bool {
        match follow_mode {
            FollowMode::Physical => false,
            FollowMode::CommandLineOnly => self.is_command_line_root,
            FollowMode::Logical => true,
        }
    }

    fn physical_view(&self) -> Result<&PlatformMetadataView, Diagnostic> {
        match self.data.physical_view.get_or_init(|| {
            self.data
                .reader
                .metadata_view(&self.path, false)
                .map_err(|error| path_error(&self.path, error))
        }) {
            Ok(view) => Ok(view),
            Err(error) => Err(error.clone()),
        }
    }

    fn logical_view(&self) -> Option<&PlatformMetadataView> {
        if self.physical_kind().ok()? != EntryKind::Symlink {
            return None;
        }

        self.data
            .logical_view
            .get_or_init(|| self.data.reader.metadata_view(&self.path, true).ok())
            .as_ref()
    }
}

pub fn file_type_to_kind(file_type: FileType) -> EntryKind {
    if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_block_device() {
        EntryKind::Block
    } else if file_type.is_char_device() {
        EntryKind::Character
    } else if file_type.is_fifo() {
        EntryKind::Fifo
    } else if file_type.is_socket() {
        EntryKind::Socket
    } else {
        EntryKind::Unknown
    }
}

fn path_error(path: &Path, error: io::Error) -> Diagnostic {
    Diagnostic::new(format!("{}: {error}", path.display()), 1)
}

#[cfg(test)]
fn read_access(path: &Path, mode: AccessMode) -> io::Result<bool> {
    crate::platform::filesystem::read_access(path, mode)
}

#[cfg(test)]
fn read_access_with(
    path: *const libc::c_char,
    mode: AccessMode,
    primary: impl FnOnce(*const libc::c_char, AccessMode) -> io::Result<bool>,
    fallback: impl FnOnce(*const libc::c_char, AccessMode) -> io::Result<bool>,
) -> io::Result<bool> {
    crate::platform::filesystem::read_access_with(path, mode, primary, fallback)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{AccessMode, EntryContext};
    use crate::platform::filesystem::{
        PlatformMetadataView, PlatformReader, metadata_view_from_metadata,
    };
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Default)]
    pub(crate) struct CountingReader {
        symlink_metadata_calls: Arc<AtomicUsize>,
        metadata_calls: Arc<AtomicUsize>,
        read_link_calls: Arc<AtomicUsize>,
        directory_probe_calls: Arc<AtomicUsize>,
        read_access_calls: Arc<AtomicUsize>,
        write_access_calls: Arc<AtomicUsize>,
        execute_access_calls: Arc<AtomicUsize>,
    }

    impl CountingReader {
        pub(crate) fn entry(
            &self,
            path: PathBuf,
            depth: usize,
            is_command_line_root: bool,
        ) -> EntryContext {
            EntryContext::new_with_reader(path, depth, is_command_line_root, Arc::new(self.clone()))
        }

        pub(crate) fn symlink_metadata_calls(&self) -> usize {
            self.symlink_metadata_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn metadata_calls(&self) -> usize {
            self.metadata_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn read_link_calls(&self) -> usize {
            self.read_link_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn directory_probe_calls(&self) -> usize {
            self.directory_probe_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn read_access_calls(&self) -> usize {
            self.read_access_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn write_access_calls(&self) -> usize {
            self.write_access_calls.load(Ordering::SeqCst)
        }

        pub(crate) fn execute_access_calls(&self) -> usize {
            self.execute_access_calls.load(Ordering::SeqCst)
        }
    }

    impl PlatformReader for CountingReader {
        fn metadata_view(&self, path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
            let metadata = if follow {
                self.metadata_calls.fetch_add(1, Ordering::SeqCst);
                std::fs::metadata(path)
            } else {
                self.symlink_metadata_calls.fetch_add(1, Ordering::SeqCst);
                std::fs::symlink_metadata(path)
            }?;
            Ok(metadata_view_from_metadata(path, &metadata, follow))
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            self.read_link_calls.fetch_add(1, Ordering::SeqCst);
            std::fs::read_link(path)
        }

        fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
            self.directory_probe_calls.fetch_add(1, Ordering::SeqCst);
            let mut entries = std::fs::read_dir(path)?;
            match entries.next() {
                None => Ok(true),
                Some(result) => result.map(|_| false),
            }
        }

        fn access(&self, path: &Path, mode: AccessMode) -> io::Result<bool> {
            match mode {
                AccessMode::Read => self.read_access_calls.fetch_add(1, Ordering::SeqCst),
                AccessMode::Write => self.write_access_calls.fetch_add(1, Ordering::SeqCst),
                AccessMode::Execute => self.execute_access_calls.fetch_add(1, Ordering::SeqCst),
            };
            super::read_access(path, mode)
        }
    }

    #[derive(Clone)]
    pub(crate) struct FakePlatformReader {
        physical_view: PlatformMetadataView,
        logical_view: Option<PlatformMetadataView>,
    }

    impl FakePlatformReader {
        pub(crate) fn with_view(view: PlatformMetadataView) -> Self {
            Self {
                physical_view: view.clone(),
                logical_view: Some(view),
            }
        }
    }

    impl PlatformReader for FakePlatformReader {
        fn metadata_view(&self, _path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
            if follow {
                self.logical_view
                    .clone()
                    .ok_or_else(|| io::Error::from_raw_os_error(libc::ENOENT))
            } else {
                Ok(self.physical_view.clone())
            }
        }

        fn read_link(&self, _path: &Path) -> io::Result<PathBuf> {
            Err(io::Error::from_raw_os_error(libc::ENOENT))
        }

        fn directory_is_empty(&self, _path: &Path) -> io::Result<bool> {
            Ok(false)
        }

        fn access(&self, _path: &Path, _mode: AccessMode) -> io::Result<bool> {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EntryContext;
    use super::test_support::CountingReader;
    use super::{AccessMode, EntryKind};
    use crate::follow::FollowMode;
    use crate::platform::filesystem::{FilesystemKey, PlatformMetadataView};
    use crate::platform::filesystem::{PlatformReader, metadata_view_from_metadata};
    use crate::time::Timestamp;
    use std::ffi::OsString;
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs as unix_fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn entry_accessors_read_from_the_platform_metadata_view() {
        let entry = EntryContext::new_with_reader(
            PathBuf::from("alpha"),
            0,
            true,
            Arc::new(super::test_support::FakePlatformReader::with_view(
                PlatformMetadataView {
                    kind: EntryKind::File,
                    identity: Some(crate::identity::FileIdentity { dev: 11, ino: 22 }),
                    size: 99,
                    owner: Some(501),
                    group: Some(20),
                    mode_bits: Some(0o640),
                    link_count: Some(2),
                    blocks_512: Some(8),
                    atime: Timestamp::new(10, 1),
                    ctime: Timestamp::new(11, 2),
                    mtime: Timestamp::new(12, 3),
                    birth_time: Some(Timestamp::new(13, 4)),
                    filesystem_key: Some(FilesystemKey(7)),
                    device_number: None,
                },
            )),
        );

        assert_eq!(entry.active_uid(FollowMode::Physical).unwrap(), 501);
        assert_eq!(entry.active_gid(FollowMode::Physical).unwrap(), 20);
        assert_eq!(entry.active_blocks(FollowMode::Physical).unwrap(), 8);
        assert_eq!(entry.active_mount_id(FollowMode::Physical).unwrap(), 7);
        assert_eq!(
            entry.active_birth_time(FollowMode::Physical).unwrap(),
            Some(Timestamp::new(13, 4))
        );
    }

    #[test]
    fn missing_optional_fields_raise_feature_specific_errors() {
        let entry = EntryContext::new_with_reader(
            PathBuf::from("alpha"),
            0,
            true,
            Arc::new(super::test_support::FakePlatformReader::with_view(
                PlatformMetadataView {
                    kind: EntryKind::File,
                    identity: Some(crate::identity::FileIdentity { dev: 11, ino: 22 }),
                    size: 99,
                    owner: None,
                    group: None,
                    mode_bits: Some(0o640),
                    link_count: Some(2),
                    blocks_512: None,
                    atime: Timestamp::new(10, 1),
                    ctime: Timestamp::new(11, 2),
                    mtime: Timestamp::new(12, 3),
                    birth_time: None,
                    filesystem_key: None,
                    device_number: None,
                },
            )),
        );

        assert!(
            entry
                .active_uid(FollowMode::Physical)
                .unwrap_err()
                .message
                .contains("owner")
        );
        assert!(
            entry
                .active_blocks(FollowMode::Physical)
                .unwrap_err()
                .message
                .contains("block")
        );
        assert!(
            entry
                .active_mount_id(FollowMode::Physical)
                .unwrap_err()
                .message
                .contains("filesystem")
        );
        assert_eq!(entry.active_birth_time(FollowMode::Physical).unwrap(), None);
    }

    #[test]
    fn basename_style_access_does_not_fetch_metadata() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);

        assert_eq!(entry.path.file_name().unwrap(), "file.txt");
        assert_eq!(reader.symlink_metadata_calls(), 0);
        assert_eq!(reader.metadata_calls(), 0);
        assert_eq!(reader.read_link_calls(), 0);
    }

    #[test]
    fn physical_metadata_is_loaded_once_and_shared_by_clones() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let clone = entry.clone();

        assert_eq!(entry.physical_kind().unwrap(), EntryKind::File);
        assert_eq!(clone.physical_kind().unwrap(), EntryKind::File);
        assert_eq!(reader.symlink_metadata_calls(), 1);
        assert_eq!(reader.metadata_calls(), 0);
    }

    #[test]
    fn logical_metadata_is_loaded_only_for_followed_view() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("real.txt"), "hello\n").unwrap();
        unix_fs::symlink(root.path().join("real.txt"), root.path().join("file-link")).unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(root.path().join("file-link"), 0, true);

        assert_eq!(
            entry.active_kind(FollowMode::Physical).unwrap(),
            EntryKind::Symlink
        );
        assert_eq!(reader.metadata_calls(), 0);

        assert_eq!(
            entry.active_kind(FollowMode::Logical).unwrap(),
            EntryKind::File
        );
        assert_eq!(
            entry.active_kind(FollowMode::Logical).unwrap(),
            EntryKind::File
        );
        assert_eq!(reader.metadata_calls(), 1);
    }

    #[test]
    fn physical_link_target_is_loaded_once() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("real.txt"), "hello\n").unwrap();
        unix_fs::symlink("real.txt", root.path().join("file-link")).unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(root.path().join("file-link"), 0, true);

        assert_eq!(
            entry
                .active_link_target(FollowMode::Physical)
                .unwrap()
                .unwrap(),
            OsString::from("real.txt")
        );
        assert_eq!(
            entry
                .active_link_target(FollowMode::Physical)
                .unwrap()
                .unwrap(),
            OsString::from("real.txt")
        );
        assert_eq!(reader.read_link_calls(), 1);
    }

    #[test]
    fn xtype_on_non_symlink_uses_physical_kind_without_logical_stat() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);

        assert_eq!(
            entry.xtype_kind(FollowMode::Physical).unwrap(),
            EntryKind::File
        );
        assert_eq!(reader.metadata_calls(), 0);
    }

    #[test]
    fn access_results_are_cached_per_mode_and_shared_by_clones() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let reader = CountingReader::default();
        let entry = reader.entry(path, 0, true);
        let clone = entry.clone();

        assert!(entry.access(AccessMode::Read).unwrap());
        assert!(clone.access(AccessMode::Read).unwrap());
        assert_eq!(reader.read_access_calls(), 1);

        assert!(entry.access(AccessMode::Write).unwrap());
        assert!(clone.access(AccessMode::Write).unwrap());
        assert_eq!(reader.write_access_calls(), 1);
        assert_eq!(reader.execute_access_calls(), 0);
    }

    #[test]
    fn access_failures_collapse_to_false() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();

        let reader = Arc::new(AccessOverrideReader {
            readable: Err(libc::ENOENT),
            writable: Ok(true),
            executable: Ok(false),
        });
        let entry = EntryContext::new_with_reader(path, 0, true, reader);

        assert!(!entry.access(AccessMode::Read).unwrap());
        assert!(entry.access(AccessMode::Write).unwrap());
        assert!(!entry.access(AccessMode::Execute).unwrap());
    }

    #[test]
    fn enosys_primary_access_path_falls_back_to_plain_access() {
        let root = tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "hello\n").unwrap();
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();

        let result = super::read_access_with(
            c_path.as_ptr(),
            AccessMode::Read,
            |_path, _mode| Err(io::Error::from_raw_os_error(libc::ENOSYS)),
            |_path, _mode| Ok(true),
        )
        .unwrap();

        assert!(result);
    }

    #[test]
    fn active_mount_id_uses_logical_root_view_only_for_command_line_only_mode() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("real")).unwrap();
        unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

        let reader = Arc::new(MountIdOverrideReader {
            physical_mount_id: 10,
            logical_mount_id: Some(20),
            fail_logical_mount_id: false,
        });
        let root_entry =
            EntryContext::new_with_reader(root.path().join("dir-link"), 0, true, reader.clone());
        let child_entry =
            EntryContext::new_with_reader(root.path().join("dir-link"), 1, false, reader);

        assert_eq!(
            root_entry.active_mount_id(FollowMode::Physical).unwrap(),
            10
        );
        assert_eq!(
            root_entry
                .active_mount_id(FollowMode::CommandLineOnly)
                .unwrap(),
            20
        );
        assert_eq!(root_entry.active_mount_id(FollowMode::Logical).unwrap(), 20);
        assert_eq!(
            child_entry
                .active_mount_id(FollowMode::CommandLineOnly)
                .unwrap(),
            10
        );
    }

    #[test]
    fn active_mount_id_reports_missing_filesystem_key_when_logical_view_lacks_it() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("real")).unwrap();
        unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

        let reader = Arc::new(MountIdOverrideReader {
            physical_mount_id: 10,
            logical_mount_id: None,
            fail_logical_mount_id: true,
        });
        let entry = EntryContext::new_with_reader(root.path().join("dir-link"), 0, true, reader);

        let error = entry.active_mount_id(FollowMode::Logical).unwrap_err();
        assert!(error.message.contains("filesystem boundary key"));
    }

    #[derive(Clone)]
    struct MountIdOverrideReader {
        physical_mount_id: u64,
        logical_mount_id: Option<u64>,
        fail_logical_mount_id: bool,
    }

    impl PlatformReader for MountIdOverrideReader {
        fn metadata_view(&self, path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
            let metadata = if follow {
                std::fs::metadata(path)
            } else {
                std::fs::symlink_metadata(path)
            }?;
            let mut view = metadata_view_from_metadata(path, &metadata, follow);
            view.filesystem_key = match (follow, self.fail_logical_mount_id, self.logical_mount_id)
            {
                (true, true, _) => None,
                (true, false, Some(mount_id)) => Some(FilesystemKey(mount_id)),
                _ => Some(FilesystemKey(self.physical_mount_id)),
            };
            Ok(view)
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            std::fs::read_link(path)
        }

        fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
            let mut entries = std::fs::read_dir(path)?;
            match entries.next() {
                None => Ok(true),
                Some(result) => result.map(|_| false),
            }
        }

        fn access(&self, path: &Path, mode: AccessMode) -> io::Result<bool> {
            super::read_access(path, mode)
        }
    }

    #[derive(Clone)]
    struct AccessOverrideReader {
        readable: Result<bool, i32>,
        writable: Result<bool, i32>,
        executable: Result<bool, i32>,
    }

    impl PlatformReader for AccessOverrideReader {
        fn metadata_view(&self, path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
            let metadata = if follow {
                std::fs::metadata(path)
            } else {
                std::fs::symlink_metadata(path)
            }?;
            Ok(metadata_view_from_metadata(path, &metadata, follow))
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            std::fs::read_link(path)
        }

        fn directory_is_empty(&self, path: &Path) -> io::Result<bool> {
            let mut entries = std::fs::read_dir(path)?;
            match entries.next() {
                None => Ok(true),
                Some(result) => result.map(|_| false),
            }
        }

        fn access(&self, _path: &Path, mode: AccessMode) -> io::Result<bool> {
            match mode {
                AccessMode::Read => self
                    .readable
                    .as_ref()
                    .copied()
                    .map_err(|errno| io::Error::from_raw_os_error(*errno)),
                AccessMode::Write => self
                    .writable
                    .as_ref()
                    .copied()
                    .map_err(|errno| io::Error::from_raw_os_error(*errno)),
                AccessMode::Execute => self
                    .executable
                    .as_ref()
                    .copied()
                    .map_err(|errno| io::Error::from_raw_os_error(*errno)),
            }
        }
    }
}
