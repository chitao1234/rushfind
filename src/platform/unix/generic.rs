#![cfg_attr(test, allow(dead_code))]

use crate::diagnostics::Diagnostic;
use crate::file_flags::FlagSpec;
use crate::platform::capabilities::OutputContract;
use crate::platform::filesystem::{FilesystemKey, FilesystemSnapshot};
use crate::platform::{PlatformCapabilities, SupportLevel};
use crate::time::Timestamp;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities {
    fstype: SupportLevel::Unsupported("`-fstype` is not supported on this platform"),
    same_file_system: SupportLevel::Exact,
    birth_time: SupportLevel::Unsupported("birth time is not supported on this platform"),
    file_flags: SupportLevel::Unsupported("`-flags` is not supported on this platform"),
    reparse_type: SupportLevel::Unsupported("reparse type is only supported on Windows"),
    named_ownership: SupportLevel::Exact,
    numeric_ownership: SupportLevel::Exact,
    windows_ownership_predicates: SupportLevel::Unsupported(
        "Windows ownership predicates are only supported on Windows",
    ),
    access_predicates: SupportLevel::Exact,
    messages_locale: SupportLevel::Approximate(
        "interactive locale behavior is approximate on this platform",
    ),
    case_insensitive_glob: SupportLevel::Approximate(
        "case-insensitive glob matching may differ outside the C locale on this platform",
    ),
    mode_bits: SupportLevel::Exact,
    output_contract: OutputContract::Posix,
};

static FLAG_SPECS: &[FlagSpec] = &[];

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &CAPABILITIES
}

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    false
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    false
}

pub(crate) fn filesystem_snapshot() -> Result<FilesystemSnapshot, Diagnostic> {
    Err(Diagnostic::unsupported(
        "`-fstype` is not supported on this platform",
    ))
}

pub(crate) fn filesystem_key(path: &Path, follow: bool) -> io::Result<FilesystemKey> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(FilesystemKey::Numeric(metadata.dev()))
}

pub(crate) fn read_file_flags(path: &Path, follow: bool) -> io::Result<Option<u64>> {
    let _ = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }?;
    Ok(None)
}

pub(crate) fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    let _ = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn generic_capabilities_are_conservative() {
        assert!(matches!(CAPABILITIES.fstype, SupportLevel::Unsupported(_)));
        assert!(matches!(CAPABILITIES.same_file_system, SupportLevel::Exact));
        assert!(matches!(
            CAPABILITIES.birth_time,
            SupportLevel::Unsupported(_)
        ));
        assert!(matches!(
            CAPABILITIES.file_flags,
            SupportLevel::Unsupported(_)
        ));
        assert!(matches!(CAPABILITIES.named_ownership, SupportLevel::Exact));
        assert!(matches!(
            CAPABILITIES.numeric_ownership,
            SupportLevel::Exact
        ));
        assert!(matches!(
            CAPABILITIES.access_predicates,
            SupportLevel::Exact
        ));
        assert!(matches!(CAPABILITIES.mode_bits, SupportLevel::Exact));
        assert!(matches!(
            CAPABILITIES.messages_locale,
            SupportLevel::Approximate(_)
        ));
        assert!(matches!(
            CAPABILITIES.case_insensitive_glob,
            SupportLevel::Approximate(_)
        ));
    }

    #[test]
    fn generic_filesystem_key_uses_st_dev() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("entry");
        fs::write(&path, b"alpha").unwrap();

        let expected = fs::metadata(&path).unwrap().dev();
        assert_eq!(
            filesystem_key(&path, true).unwrap(),
            FilesystemKey::Numeric(expected)
        );
    }

    #[test]
    fn generic_optional_metadata_surfaces_return_none_for_existing_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("entry");
        fs::write(&path, b"alpha").unwrap();

        assert_eq!(read_file_flags(&path, true).unwrap(), None);
        assert_eq!(read_birth_time(&path, true).unwrap(), None);
    }
}
