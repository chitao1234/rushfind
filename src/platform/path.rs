#![cfg_attr(windows, allow(dead_code))]

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub(crate) fn display_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        return path.as_os_str().as_encoded_bytes().to_vec();
    }

    #[cfg(windows)]
    {
        return path.display().to_string().replace('/', "\\").into_bytes();
    }
}

pub(crate) fn display_os_bytes(value: &OsStr) -> Vec<u8> {
    display_bytes(Path::new(value))
}

#[allow(dead_code)]
pub(crate) fn match_bytes(path: &Path) -> Vec<u8> {
    normalize_match_bytes(path.as_os_str())
}

pub(crate) fn encoded_bytes(value: &OsStr) -> &[u8] {
    value.as_encoded_bytes()
}

pub(crate) fn os_string_from_encoded_bytes(bytes: Vec<u8>) -> OsString {
    unsafe { OsString::from_encoded_bytes_unchecked(bytes) }
}

pub(crate) fn normalize_match_text<'a>(value: &'a OsStr) -> Cow<'a, OsStr> {
    #[cfg(unix)]
    {
        Cow::Borrowed(value)
    }

    #[cfg(windows)]
    {
        let bytes = encoded_bytes(value);
        if !bytes.contains(&b'\\') {
            Cow::Borrowed(value)
        } else {
            Cow::Owned(os_string_from_encoded_bytes(normalize_match_bytes(value)))
        }
    }
}

pub(crate) fn execdir_placeholder(path: &Path) -> OsString {
    let basename = path.file_name().unwrap_or_else(|| OsStr::new(""));

    #[cfg(unix)]
    {
        let mut bytes = b"./".to_vec();
        bytes.extend_from_slice(encoded_bytes(basename));
        return os_string_from_encoded_bytes(bytes);
    }

    #[cfg(windows)]
    {
        let mut wide = ".\\".encode_utf16().collect::<Vec<_>>();
        wide.extend(basename.encode_wide());
        return OsString::from_wide(&wide);
    }
}

pub(crate) fn relative_dir_for_printf(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn normalize_match_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        return encoded_bytes(value).to_vec();
    }

    #[cfg(windows)]
    {
        return encoded_bytes(value)
            .iter()
            .map(|byte| if *byte == b'\\' { b'/' } else { *byte })
            .collect();
    }
}
