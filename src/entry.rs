use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use std::fs::{FileType, Metadata};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::PathBuf;

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

#[derive(Debug, Clone)]
pub struct EntryContext {
    pub path: PathBuf,
    pub depth: usize,
    pub is_command_line_root: bool,
    pub physical_metadata: Metadata,
    pub logical_metadata: Option<Metadata>,
}

impl EntryContext {
    pub fn new(
        path: PathBuf,
        depth: usize,
        is_command_line_root: bool,
        physical_metadata: Metadata,
        logical_metadata: Option<Metadata>,
    ) -> Self {
        Self {
            path,
            depth,
            is_command_line_root,
            physical_metadata,
            logical_metadata,
        }
    }

    pub fn physical_kind(&self) -> EntryKind {
        file_type_to_kind(self.physical_metadata.file_type())
    }

    pub fn physical_identity(&self) -> FileIdentity {
        FileIdentity::from_metadata(&self.physical_metadata)
    }

    pub fn active_metadata(&self, follow_mode: FollowMode) -> &Metadata {
        match follow_mode {
            FollowMode::Physical => &self.physical_metadata,
            FollowMode::CommandLineOnly if self.is_command_line_root => {
                self.logical_metadata
                    .as_ref()
                    .unwrap_or(&self.physical_metadata)
            }
            FollowMode::CommandLineOnly => &self.physical_metadata,
            FollowMode::Logical => self.logical_metadata.as_ref().unwrap_or(&self.physical_metadata),
        }
    }

    pub fn logical_kind(&self) -> EntryKind {
        self.logical_metadata
            .as_ref()
            .map(|metadata| file_type_to_kind(metadata.file_type()))
            .unwrap_or_else(|| self.physical_kind())
    }

    pub fn logical_identity(&self) -> Option<FileIdentity> {
        self.logical_metadata
            .as_ref()
            .map(FileIdentity::from_metadata)
    }

    pub fn active_identity(&self, follow_mode: FollowMode) -> FileIdentity {
        FileIdentity::from_metadata(self.active_metadata(follow_mode))
    }

    pub fn active_inode(&self, follow_mode: FollowMode) -> u64 {
        self.active_metadata(follow_mode).ino()
    }

    pub fn active_link_count(&self, follow_mode: FollowMode) -> u64 {
        self.active_metadata(follow_mode).nlink()
    }

    pub fn active_kind(&self, follow_mode: FollowMode) -> EntryKind {
        file_type_to_kind(self.active_metadata(follow_mode).file_type())
    }

    pub fn active_directory_identity(&self, follow_mode: FollowMode) -> Option<FileIdentity> {
        if self.active_kind(follow_mode) != EntryKind::Directory {
            return None;
        }

        Some(self.active_identity(follow_mode))
    }

    pub fn xtype_kind(&self, follow_mode: FollowMode) -> EntryKind {
        match follow_mode {
            FollowMode::Logical => self.physical_kind(),
            FollowMode::Physical | FollowMode::CommandLineOnly => self.logical_kind(),
        }
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
