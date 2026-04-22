use crate::diagnostics::Diagnostic;
use crate::entry::{AccessMode, EntryKind};
use crate::identity::FileIdentity;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot, PlatformMetadataView};
use crate::time::Timestamp;
use std::ffi::OsString;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, FileBasicInfo,
    FileIdInfo, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetVolumeInformationW, OPEN_EXISTING,
};

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000000C;
const WINDOWS_TICKS_PER_SECOND: i64 = 10_000_000;
const WINDOWS_TO_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;
const VOLUME_BUFFER_LEN: usize = 1024;
const FILESYSTEM_NAME_BUFFER_LEN: usize = 256;

pub(crate) fn metadata_view(path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
    let handle = MetadataHandle::open(path, follow)?;
    let basic = query_basic_info(handle.raw())?;
    let attributes = query_attribute_tag_info(handle.raw())?;
    let handle_info = query_handle_info(handle.raw())?;
    let (volume_serial, file_id) = query_identity(handle.raw(), &handle_info)?;

    Ok(PlatformMetadataView {
        kind: classify_kind(&attributes, follow),
        identity: Some(FileIdentity::Windows {
            volume_serial,
            file_id,
        }),
        size: combine_u32(handle_info.nFileSizeHigh, handle_info.nFileSizeLow),
        owner: None,
        group: None,
        mode_bits: None,
        native_attributes: Some(attributes.FileAttributes),
        reparse_tag: reparse_tag(&attributes),
        link_count: Some(handle_info.nNumberOfLinks as u64),
        blocks_512: None,
        atime: filetime_to_timestamp(basic.LastAccessTime)?,
        ctime: filetime_to_timestamp(basic.ChangeTime)?,
        mtime: filetime_to_timestamp(basic.LastWriteTime)?,
        birth_time: filetime_to_optional_timestamp(basic.CreationTime)?,
        filesystem_key: Some(FilesystemKey::Numeric(volume_serial)),
        device_number: None,
    })
}

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    let mut snapshot = FilesystemSnapshot::default();
    let finder = VolumeFinder::start()
        .map_err(|error| Diagnostic::new(format!("failed to enumerate volumes: {error}"), 1))?;

    for volume_path in finder {
        let Ok((serial, filesystem_name)) = query_volume_information(&volume_path) else {
            continue;
        };
        snapshot.insert(FilesystemKey::Numeric(serial), filesystem_name);
    }

    Ok(snapshot)
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let handle = MetadataHandle::open(path, follow)?;
    let handle_info = query_handle_info(handle.raw())?;
    let (volume_serial, _) = query_identity(handle.raw(), &handle_info)?;
    Ok(FilesystemKey::Numeric(volume_serial))
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let handle = MetadataHandle::open(path, follow)
        .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;
    let basic = query_basic_info(handle.raw())
        .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;
    filetime_to_optional_timestamp(basic.CreationTime)
        .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))
}

pub(crate) fn read_access(_path: &Path, _mode: AccessMode) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "access predicates are not implemented on Windows yet",
    ))
}

struct MetadataHandle(HANDLE);

impl MetadataHandle {
    fn open(path: &Path, follow: bool) -> io::Result<Self> {
        let path_wide = wide_null(path);
        let flags = FILE_FLAG_BACKUP_SEMANTICS
            | if follow {
                0
            } else {
                FILE_FLAG_OPEN_REPARSE_POINT
            };
        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null_mut(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for MetadataHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct VolumeFinder {
    handle: HANDLE,
    first: Option<Vec<u16>>,
    buffer: Vec<u16>,
    finished: bool,
}

impl VolumeFinder {
    fn start() -> io::Result<Self> {
        let mut buffer = vec![0u16; VOLUME_BUFFER_LEN];
        let handle = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            handle,
            first: Some(trim_wide(&buffer).to_vec()),
            buffer,
            finished: false,
        })
    }
}

impl Iterator for VolumeFinder {
    type Item = Vec<u16>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first) = self.first.take() {
            return Some(first);
        }
        if self.finished {
            return None;
        }

        self.buffer.fill(0);
        let ok = unsafe {
            FindNextVolumeW(
                self.handle,
                self.buffer.as_mut_ptr(),
                self.buffer.len() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            self.finished = true;
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return None;
            }
            return None;
        }

        Some(trim_wide(&self.buffer).to_vec())
    }
}

impl Drop for VolumeFinder {
    fn drop(&mut self) {
        unsafe {
            let _ = FindVolumeClose(self.handle);
        }
    }
}

fn query_basic_info(handle: HANDLE) -> io::Result<FILE_BASIC_INFO> {
    let mut info = FILE_BASIC_INFO::default();
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&mut info as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn query_attribute_tag_info(handle: HANDLE) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn query_handle_info(handle: HANDLE) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn query_id_info(handle: HANDLE) -> io::Result<FILE_ID_INFO> {
    let mut info = FILE_ID_INFO::default();
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

fn query_identity(
    handle: HANDLE,
    handle_info: &BY_HANDLE_FILE_INFORMATION,
) -> io::Result<(u64, u128)> {
    match query_id_info(handle) {
        Ok(info) => Ok((
            info.VolumeSerialNumber,
            u128::from_le_bytes(info.FileId.Identifier),
        )),
        Err(_) => Ok((
            handle_info.dwVolumeSerialNumber as u64,
            combine_u32(handle_info.nFileIndexHigh, handle_info.nFileIndexLow) as u128,
        )),
    }
}

fn query_volume_information(volume_path: &[u16]) -> io::Result<(u64, OsString)> {
    let mut serial = 0_u32;
    let mut filesystem_name = vec![0u16; FILESYSTEM_NAME_BUFFER_LEN];
    let volume_path = null_terminated(volume_path);
    let ok = unsafe {
        GetVolumeInformationW(
            volume_path.as_ptr(),
            null_mut(),
            0,
            &mut serial,
            null_mut(),
            null_mut(),
            filesystem_name.as_mut_ptr(),
            filesystem_name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    Ok((
        serial as u64,
        OsString::from_wide(trim_wide(&filesystem_name)),
    ))
}

fn classify_kind(attributes: &FILE_ATTRIBUTE_TAG_INFO, follow: bool) -> EntryKind {
    if !follow {
        match reparse_tag(attributes) {
            Some(IO_REPARSE_TAG_SYMLINK) => return EntryKind::Symlink,
            Some(IO_REPARSE_TAG_MOUNT_POINT) => return EntryKind::Directory,
            _ => {}
        }
    }

    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

fn reparse_tag(attributes: &FILE_ATTRIBUTE_TAG_INFO) -> Option<u32> {
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Some(attributes.ReparseTag)
    } else {
        None
    }
}

fn filetime_to_timestamp(filetime: i64) -> io::Result<Timestamp> {
    let ticks_since_epoch = filetime
        .checked_sub(WINDOWS_TO_UNIX_EPOCH_SECONDS * WINDOWS_TICKS_PER_SECOND)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "timestamp underflow"))?;
    let seconds = ticks_since_epoch.div_euclid(WINDOWS_TICKS_PER_SECOND);
    let nanos = (ticks_since_epoch.rem_euclid(WINDOWS_TICKS_PER_SECOND) * 100) as i32;
    Ok(Timestamp::new(seconds, nanos))
}

fn filetime_to_optional_timestamp(filetime: i64) -> io::Result<Option<Timestamp>> {
    if filetime == 0 {
        Ok(None)
    } else {
        filetime_to_timestamp(filetime).map(Some)
    }
}

fn combine_u32(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn wide_null(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn null_terminated(value: &[u16]) -> Vec<u16> {
    let mut owned = value.to_vec();
    if owned.last().copied() != Some(0) {
        owned.push(0);
    }
    owned
}

fn trim_wide(buffer: &[u16]) -> &[u16] {
    let len = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    &buffer[..len]
}
