use crate::diagnostics::Diagnostic;
use crate::entry::{AccessMode, EntryKind};
use crate::identity::FileIdentity;
use crate::platform::filesystem::{
    FilesystemKey, FilesystemSnapshot, PlatformMetadataView, PlatformPrincipalId,
};
use crate::time::Timestamp;
use std::ffi::OsString;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
    LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSID, SecurityImpersonation, TOKEN_DUPLICATE,
    TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_EXECUTE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, FileBasicInfo, FileIdInfo,
    FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetVolumeInformationW, OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000000C;
const WINDOWS_TICKS_PER_SECOND: i64 = 10_000_000;
const WINDOWS_TO_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;
const VOLUME_BUFFER_LEN: usize = 1024;
const FILESYSTEM_NAME_BUFFER_LEN: usize = 256;
const SECURITY_QUERY_ACCESS: u32 = FILE_READ_ATTRIBUTES | READ_CONTROL;

pub(crate) fn metadata_view(path: &Path, follow: bool) -> io::Result<PlatformMetadataView> {
    let handle = MetadataHandle::open(path, follow)?;
    let basic = query_basic_info(handle.raw())?;
    let attributes = query_attribute_tag_info(handle.raw())?;
    let handle_info = query_handle_info(handle.raw())?;
    let (volume_serial, file_id) = query_identity(handle.raw(), &handle_info)?;
    let (owner, group) = query_owner_and_group(handle.raw()).unwrap_or((None, None));

    Ok(PlatformMetadataView {
        kind: classify_kind(&attributes, follow),
        identity: Some(FileIdentity::Windows {
            volume_serial,
            file_id,
        }),
        size: combine_u32(handle_info.nFileSizeHigh, handle_info.nFileSizeLow),
        owner,
        group,
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

pub(crate) fn read_access(path: &Path, mode: AccessMode) -> io::Result<bool> {
    let handle = MetadataHandle::open_with_access(path, true, SECURITY_QUERY_ACCESS, None)?;
    let descriptor = SecurityDescriptor::query(
        handle.raw(),
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
    )?;
    let token = TokenHandle::open_for_access_check()?;

    let desired_access = match mode {
        AccessMode::Read => FILE_GENERIC_READ,
        AccessMode::Write => FILE_GENERIC_WRITE,
        AccessMode::Execute => FILE_GENERIC_EXECUTE,
    };
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };

    let mut privileges = vec![0u8; size_of::<PRIVILEGE_SET>()];
    let mut privileges_len = privileges.len() as u32;
    loop {
        let mut granted = 0u32;
        let mut status = 0i32;
        if unsafe {
            AccessCheck(
                descriptor.raw(),
                token.raw(),
                desired_access,
                &mapping,
                privileges.as_mut_ptr().cast(),
                &mut privileges_len,
                &mut granted,
                &mut status,
            )
        } != 0
        {
            return Ok(status != 0);
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            privileges.resize(privileges_len as usize, 0);
            continue;
        }
        return Err(error);
    }
}

struct MetadataHandle(HANDLE);

impl MetadataHandle {
    fn open(path: &Path, follow: bool) -> io::Result<Self> {
        Self::open_with_access(
            path,
            follow,
            SECURITY_QUERY_ACCESS,
            Some(FILE_READ_ATTRIBUTES),
        )
    }

    fn open_with_access(
        path: &Path,
        follow: bool,
        desired_access: u32,
        fallback_access: Option<u32>,
    ) -> io::Result<Self> {
        let path_wide = wide_null(path);
        let flags = FILE_FLAG_BACKUP_SEMANTICS
            | if follow {
                0
            } else {
                FILE_FLAG_OPEN_REPARSE_POINT
            };
        let handle = open_handle(&path_wide, desired_access, flags).or_else(|error| {
            let Some(fallback_access) = fallback_access else {
                return Err(error);
            };
            open_handle(&path_wide, fallback_access, flags)
        })?;
        Ok(Self(handle))
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

fn open_handle(path_wide: &[u16], desired_access: u32, flags: u32) -> io::Result<HANDLE> {
    let handle = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            desired_access,
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
        Ok(handle)
    }
}

impl Drop for MetadataHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct TokenHandle(HANDLE);

impl TokenHandle {
    fn open_for_access_check() -> io::Result<Self> {
        let mut thread_token: HANDLE = null_mut();
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut thread_token) } != 0 {
            return Ok(Self(thread_token));
        }

        let mut process_token: HANDLE = null_mut();
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut process_token,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process_token = Self(process_token);

        let mut impersonation_token: HANDLE = null_mut();
        if unsafe {
            windows_sys::Win32::Security::DuplicateToken(
                process_token.raw(),
                SecurityImpersonation,
                &mut impersonation_token,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        Ok(Self(impersonation_token))
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for TokenHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct SecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn query(
        handle: HANDLE,
        security_info: windows_sys::Win32::Security::OBJECT_SECURITY_INFORMATION,
    ) -> io::Result<Self> {
        let mut descriptor = null_mut();
        let result = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                security_info,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        if result != 0 {
            Err(io::Error::from_raw_os_error(result as i32))
        } else {
            Ok(Self(descriptor))
        }
    }

    fn raw(&self) -> windows_sys::Win32::Security::PSECURITY_DESCRIPTOR {
        self.0
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0.cast());
            }
        }
    }
}

fn query_owner_and_group(
    handle: HANDLE,
) -> io::Result<(Option<PlatformPrincipalId>, Option<PlatformPrincipalId>)> {
    let mut owner = null_mut();
    let mut group = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION,
            &mut owner,
            &mut group,
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }

    let descriptor = SecurityDescriptor(descriptor);
    let owner = principal_from_sid(owner)?;
    let group = principal_from_sid(group)?;
    drop(descriptor);
    Ok((owner, group))
}

fn principal_from_sid(sid: PSID) -> io::Result<Option<PlatformPrincipalId>> {
    if sid.is_null() {
        return Ok(None);
    }

    Ok(Some(PlatformPrincipalId::Sid(sid_to_string(sid)?)))
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut raw = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let owned = LocalWideString(raw);
    Ok(String::from_utf16_lossy(owned.as_slice()))
}

struct LocalWideString(*mut u16);

impl LocalWideString {
    fn as_slice(&self) -> &[u16] {
        if self.0.is_null() {
            return &[];
        }

        let mut len = 0usize;
        unsafe {
            while *self.0.add(len) != 0 {
                len += 1;
            }
            std::slice::from_raw_parts(self.0, len)
        }
    }
}

impl Drop for LocalWideString {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = LocalFree(self.0.cast());
            }
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
