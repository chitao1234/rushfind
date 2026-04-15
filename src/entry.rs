use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::time::Timestamp;
use std::ffi::OsString;
use std::fmt;
use std::fs::{FileType, Metadata};
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
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

pub(crate) trait EntryReader: Send + Sync {
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata>;
    fn metadata(&self, path: &Path) -> io::Result<Metadata>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn directory_is_empty(&self, path: &Path) -> io::Result<bool>;
}

#[derive(Debug)]
struct FsEntryReader;

impl EntryReader for FsEntryReader {
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        std::fs::symlink_metadata(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        std::fs::metadata(path)
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
}

#[derive(Clone)]
pub struct EntryContext {
    pub path: PathBuf,
    pub depth: usize,
    pub is_command_line_root: bool,
    data: Arc<EntryData>,
}

struct EntryData {
    reader: Arc<dyn EntryReader>,
    physical_file_type_hint: Option<FileType>,
    physical_metadata: OnceLock<Result<Metadata, Diagnostic>>,
    logical_metadata: OnceLock<Option<Metadata>>,
    physical_identity: OnceLock<Result<FileIdentity, Diagnostic>>,
    logical_identity: OnceLock<Option<FileIdentity>>,
    physical_link_target: OnceLock<Result<Option<OsString>, Diagnostic>>,
    active_directory_empty: OnceLock<Result<bool, Diagnostic>>,
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
        Self::with_reader_and_hint(
            path,
            depth,
            is_command_line_root,
            None,
            Arc::new(FsEntryReader),
        )
    }

    pub(crate) fn with_file_type_hint(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        physical_file_type_hint: Option<FileType>,
    ) -> Self {
        Self::with_reader_and_hint(
            path,
            depth,
            is_command_line_root,
            physical_file_type_hint,
            Arc::new(FsEntryReader),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_reader(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        reader: Arc<dyn EntryReader>,
    ) -> Self {
        Self::with_reader_and_hint(path, depth, is_command_line_root, None, reader)
    }

    fn with_reader_and_hint(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        physical_file_type_hint: Option<FileType>,
        reader: Arc<dyn EntryReader>,
    ) -> Self {
        Self {
            path,
            depth,
            is_command_line_root,
            data: Arc::new(EntryData {
                reader,
                physical_file_type_hint,
                physical_metadata: OnceLock::new(),
                logical_metadata: OnceLock::new(),
                physical_identity: OnceLock::new(),
                logical_identity: OnceLock::new(),
                physical_link_target: OnceLock::new(),
                active_directory_empty: OnceLock::new(),
            }),
        }
    }

    pub fn physical_kind(&self) -> Result<EntryKind, Diagnostic> {
        if let Some(file_type) = self.data.physical_file_type_hint {
            return Ok(file_type_to_kind(file_type));
        }

        Ok(file_type_to_kind(self.physical_metadata()?.file_type()))
    }

    pub fn physical_identity(&self) -> Result<FileIdentity, Diagnostic> {
        match self
            .data
            .physical_identity
            .get_or_init(|| self.physical_metadata().map(FileIdentity::from_metadata))
        {
            Ok(identity) => Ok(*identity),
            Err(error) => Err(error.clone()),
        }
    }

    pub fn active_metadata(&self, follow_mode: FollowMode) -> Result<&Metadata, Diagnostic> {
        if self.uses_logical_view(follow_mode) && self.physical_kind()? == EntryKind::Symlink {
            if let Some(metadata) = self.logical_metadata() {
                return Ok(metadata);
            }
        }

        self.physical_metadata()
    }

    pub fn logical_kind(&self) -> Result<EntryKind, Diagnostic> {
        if self.physical_kind()? != EntryKind::Symlink {
            return self.physical_kind();
        }

        if let Some(metadata) = self.logical_metadata() {
            Ok(file_type_to_kind(metadata.file_type()))
        } else {
            self.physical_kind()
        }
    }

    pub fn logical_identity(&self) -> Option<FileIdentity> {
        *self.data.logical_identity.get_or_init(|| {
            if self.physical_kind().ok()? != EntryKind::Symlink {
                return self.physical_identity().ok();
            }

            self.logical_metadata().map(FileIdentity::from_metadata)
        })
    }

    pub fn active_identity(&self, follow_mode: FollowMode) -> Result<FileIdentity, Diagnostic> {
        if self.uses_logical_view(follow_mode) && self.physical_kind()? == EntryKind::Symlink {
            if let Some(identity) = self.logical_identity() {
                return Ok(identity);
            }
        }

        self.physical_identity()
    }

    pub fn active_inode(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_identity(follow_mode)?.ino)
    }

    pub fn active_uid(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        Ok(self.active_metadata(follow_mode)?.uid())
    }

    pub fn active_gid(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        Ok(self.active_metadata(follow_mode)?.gid())
    }

    pub fn active_mode_bits(&self, follow_mode: FollowMode) -> Result<u32, Diagnostic> {
        Ok(self.active_metadata(follow_mode)?.mode() & 0o7777)
    }

    pub fn active_size(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_metadata(follow_mode)?.len())
    }

    pub fn active_atime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        let metadata = self.active_metadata(follow_mode)?;
        Ok(Timestamp::new(
            metadata.atime(),
            metadata.atime_nsec() as i32,
        ))
    }

    pub fn active_ctime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        let metadata = self.active_metadata(follow_mode)?;
        Ok(Timestamp::new(
            metadata.ctime(),
            metadata.ctime_nsec() as i32,
        ))
    }

    pub fn active_mtime(&self, follow_mode: FollowMode) -> Result<Timestamp, Diagnostic> {
        let metadata = self.active_metadata(follow_mode)?;
        Ok(Timestamp::new(
            metadata.mtime(),
            metadata.mtime_nsec() as i32,
        ))
    }

    pub fn active_link_count(&self, follow_mode: FollowMode) -> Result<u64, Diagnostic> {
        Ok(self.active_metadata(follow_mode)?.nlink())
    }

    pub fn active_is_empty(&self, follow_mode: FollowMode) -> Result<bool, Diagnostic> {
        match self.active_kind(follow_mode)? {
            EntryKind::File => Ok(self.active_size(follow_mode)? == 0),
            EntryKind::Directory => match self.data.active_directory_empty.get_or_init(|| {
                self.data
                    .reader
                    .directory_is_empty(&self.path)
                    .map_err(|error| path_error(&self.path, error))
            }) {
                Ok(is_empty) => Ok(*is_empty),
                Err(error) => Err(error.clone()),
            },
            _ => Ok(false),
        }
    }

    pub fn active_kind(&self, follow_mode: FollowMode) -> Result<EntryKind, Diagnostic> {
        if self.uses_logical_view(follow_mode) && self.physical_kind()? == EntryKind::Symlink {
            if let Some(metadata) = self.logical_metadata() {
                return Ok(file_type_to_kind(metadata.file_type()));
            }
        }

        self.physical_kind()
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
                if self.logical_metadata().is_some() {
                    Ok(None)
                } else {
                    self.physical_link_target()
                }
            }
            FollowMode::CommandLineOnly => self.physical_link_target(),
            FollowMode::Logical => {
                if self.logical_metadata().is_some() {
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

    fn physical_metadata(&self) -> Result<&Metadata, Diagnostic> {
        match self.data.physical_metadata.get_or_init(|| {
            self.data
                .reader
                .symlink_metadata(&self.path)
                .map_err(|error| path_error(&self.path, error))
        }) {
            Ok(metadata) => Ok(metadata),
            Err(error) => Err(error.clone()),
        }
    }

    fn logical_metadata(&self) -> Option<&Metadata> {
        if self.physical_kind().ok()? != EntryKind::Symlink {
            return None;
        }

        self.data
            .logical_metadata
            .get_or_init(|| self.data.reader.metadata(&self.path).ok())
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
pub(crate) mod test_support {
    use super::{EntryContext, EntryReader};
    use std::fs::Metadata;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Default)]
    pub(crate) struct CountingReader {
        symlink_metadata_calls: Arc<AtomicUsize>,
        metadata_calls: Arc<AtomicUsize>,
        read_link_calls: Arc<AtomicUsize>,
        directory_probe_calls: Arc<AtomicUsize>,
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
    }

    impl EntryReader for CountingReader {
        fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
            self.symlink_metadata_calls.fetch_add(1, Ordering::SeqCst);
            std::fs::symlink_metadata(path)
        }

        fn metadata(&self, path: &Path) -> io::Result<Metadata> {
            self.metadata_calls.fetch_add(1, Ordering::SeqCst);
            std::fs::metadata(path)
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
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::CountingReader;
    use super::EntryKind;
    use crate::follow::FollowMode;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs as unix_fs;
    use tempfile::tempdir;

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
}
