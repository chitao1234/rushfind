use std::fs::Metadata;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileIdentity {
    Unix { dev: u64, ino: u64 },
    Windows { volume_serial: u64, file_id: u128 },
}

impl FileIdentity {
    #[cfg(unix)]
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self::Unix {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }

    #[cfg(windows)]
    pub fn from_metadata(_metadata: &Metadata) -> Self {
        unimplemented!("windows file identity is not implemented yet")
    }

    pub fn device_number(self) -> u64 {
        match self {
            Self::Unix { dev, .. } => dev,
            Self::Windows { volume_serial, .. } => volume_serial,
        }
    }

    pub fn inode_number(self) -> u64 {
        match self {
            Self::Unix { ino, .. } => ino,
            Self::Windows { file_id, .. } => file_id as u64,
        }
    }
}
