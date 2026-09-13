#![cfg_attr(windows, allow(dead_code))]

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub(crate) fn display_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        path.as_os_str().as_encoded_bytes().to_vec()
    }

    #[cfg(windows)]
    {
        path.display().to_string().replace('/', "\\").into_bytes()
    }
}

/// Display bytes with a terminator appended in the same allocation. Every
/// `-print*` action needs exactly this shape, and building it in one pass keeps
/// the hot output path down to a single allocation per record.
pub(crate) fn display_bytes_with_terminator(path: &Path, terminator: u8) -> Vec<u8> {
    #[cfg(unix)]
    {
        let raw = path.as_os_str().as_encoded_bytes();
        let mut bytes = Vec::with_capacity(raw.len() + 1);
        bytes.extend_from_slice(raw);
        bytes.push(terminator);
        bytes
    }

    #[cfg(windows)]
    {
        let mut bytes = display_bytes(path);
        bytes.push(terminator);
        bytes
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

pub(crate) fn normalize_match_text(value: &OsStr) -> Cow<'_, OsStr> {
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
    let base = base_name_slice(encoded_bytes(path.as_os_str()));

    // find drops the "./" prefix when the base name is a file system root.
    if base.first().is_some_and(|byte| is_separator(*byte)) {
        return os_string_from_encoded_bytes(base.to_vec());
    }

    #[cfg(unix)]
    {
        let mut bytes = b"./".to_vec();
        bytes.extend_from_slice(base);
        os_string_from_encoded_bytes(bytes)
    }

    #[cfg(windows)]
    {
        let mut wide = ".\\".encode_utf16().collect::<Vec<_>>();
        wide.extend(os_string_from_encoded_bytes(base.to_vec()).encode_wide());
        OsString::from_wide(&wide)
    }
}

/// `%f`: gnulib `base_name()`, in display form.
pub(crate) fn display_base_name(path: &Path) -> Vec<u8> {
    base_name_slice(&display_bytes(path)).to_vec()
}

/// `-name`/`-iname`: gnulib `base_name()` followed by
/// `strip_trailing_slashes()`, in match form.
pub(crate) fn match_base_name(value: &OsStr) -> Cow<'_, OsStr> {
    let base = without_trailing_separators(base_name_slice(encoded_bytes(value)));

    // SAFETY: both helpers cut at single-byte separators, so the slice keeps
    // the boundaries of the original encoding.
    normalize_match_text(unsafe { OsStr::from_encoded_bytes_unchecked(base) })
}

/// `%h`: find's own leading-directories rule, in display form.  find does not
/// use gnulib `dir_name()` here: it drops trailing separators (unless the whole
/// path is separators), then cuts at the last remaining separator.
pub(crate) fn relative_dir_for_printf(path: &Path) -> PathBuf {
    PathBuf::from(os_string_from_encoded_bytes(
        dir_name_slice(&display_bytes(path)).to_vec(),
    ))
}

/// gnulib `base_name()`: the last path component, with a run of trailing
/// separators collapsed into one.  A file system root comes back whole, so
/// "/" stays "/" and "a/" stays "a/".
fn base_name_slice(bytes: &[u8]) -> &[u8] {
    let start = last_component_start(bytes);

    // No component at all: the name is a file system root or empty.
    if start == bytes.len() {
        return &bytes[..base_len(bytes)];
    }

    let mut len = base_len(&bytes[start..]);
    if start + len < bytes.len() && is_separator(bytes[start + len]) {
        len += 1;
    }

    &bytes[start..start + len]
}

/// gnulib `strip_trailing_slashes()`: drop trailing separators, but keep a bare
/// root ("/", "///") as it is.
fn without_trailing_separators(bytes: &[u8]) -> &[u8] {
    let start = last_component_start(bytes);
    let start = if start == bytes.len() { 0 } else { start };

    &bytes[..start + base_len(&bytes[start..])]
}

/// find's `%h`: cut at the last separator that survives the trailing-slash
/// strip, or "." when the path holds no separator.
fn dir_name_slice(bytes: &[u8]) -> &[u8] {
    let end = match bytes.iter().rposition(|byte| !is_separator(*byte)) {
        // A path that is all separators, or that keeps only its leading one
        // ("a/"), stays whole: the cut below still finds a separator to use.
        None | Some(0) => bytes.len(),
        Some(last_non_separator) => last_non_separator + 1,
    };

    match bytes[..end].iter().rposition(|byte| is_separator(*byte)) {
        Some(cut) => &bytes[..cut],
        None => b".",
    }
}

/// gnulib `last_component()`: offset of the last path component, past any
/// leading separators.  `bytes.len()` means the name has no component, i.e. it
/// is empty or a file system root.
fn last_component_start(bytes: &[u8]) -> usize {
    let mut start = 0;
    while start < bytes.len() && is_separator(bytes[start]) {
        start += 1;
    }

    let mut last_was_separator = false;
    for index in start..bytes.len() {
        if is_separator(bytes[index]) {
            last_was_separator = true;
        } else if last_was_separator {
            start = index;
            last_was_separator = false;
        }
    }

    start
}

/// gnulib `base_len()`: length of a component ignoring trailing separators,
/// never shorter than one byte.  gnulib's `DOUBLE_SLASH_IS_DISTINCT_ROOT` is 0
/// on every platform rushfind targets, so "//" is not a distinct root here.
fn base_len(bytes: &[u8]) -> usize {
    let mut len = bytes.len();
    while 1 < len && is_separator(bytes[len - 1]) {
        len -= 1;
    }

    len
}

/// gnulib's `ISSLASH`: Windows accepts both separators, every other platform
/// only '/'.  The display and match helpers normalize to one of the two, so the
/// byte algorithms above accept either.
#[inline]
fn is_separator(byte: u8) -> bool {
    #[cfg(unix)]
    {
        byte == b'/'
    }

    #[cfg(windows)]
    {
        byte == b'/' || byte == b'\\'
    }
}

fn normalize_match_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        encoded_bytes(value).to_vec()
    }

    #[cfg(windows)]
    {
        encoded_bytes(value)
            .iter()
            .map(|byte| if *byte == b'\\' { b'/' } else { *byte })
            .collect()
    }
}
