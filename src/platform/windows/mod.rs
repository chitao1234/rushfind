pub(crate) mod accounts;
pub(crate) mod filesystem;
pub(crate) mod locale;

use crate::file_flags::FlagSpec;
use crate::platform::{PlatformCapabilities, SupportLevel};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_ENCRYPTED,
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_INTEGRITY_STREAM, FILE_ATTRIBUTE_NO_SCRUB_DATA,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_PINNED, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
    FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE,
    FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_TEMPORARY, FILE_ATTRIBUTE_UNPINNED,
};

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Unsupported("numeric ownership is not supported on Windows"),
    SupportLevel::Exact,
    SupportLevel::Approximate("interactive locale behavior is approximate on Windows"),
    SupportLevel::Approximate(
        "case-insensitive glob matching may differ outside the C locale on Windows",
    ),
    SupportLevel::Unsupported("Unix mode bits are not supported on Windows"),
);

pub(crate) static FLAG_SPECS: &[FlagSpec] = &[
    FlagSpec {
        name: "archive",
        bit: FILE_ATTRIBUTE_ARCHIVE as u64,
    },
    FlagSpec {
        name: "compressed",
        bit: FILE_ATTRIBUTE_COMPRESSED as u64,
    },
    FlagSpec {
        name: "encrypted",
        bit: FILE_ATTRIBUTE_ENCRYPTED as u64,
    },
    FlagSpec {
        name: "hidden",
        bit: FILE_ATTRIBUTE_HIDDEN as u64,
    },
    FlagSpec {
        name: "integrity-stream",
        bit: FILE_ATTRIBUTE_INTEGRITY_STREAM as u64,
    },
    FlagSpec {
        name: "normal",
        bit: FILE_ATTRIBUTE_NORMAL as u64,
    },
    FlagSpec {
        name: "not-content-indexed",
        bit: FILE_ATTRIBUTE_NOT_CONTENT_INDEXED as u64,
    },
    FlagSpec {
        name: "no-scrub-data",
        bit: FILE_ATTRIBUTE_NO_SCRUB_DATA as u64,
    },
    FlagSpec {
        name: "offline",
        bit: FILE_ATTRIBUTE_OFFLINE as u64,
    },
    FlagSpec {
        name: "pinned",
        bit: FILE_ATTRIBUTE_PINNED as u64,
    },
    FlagSpec {
        name: "readonly",
        bit: FILE_ATTRIBUTE_READONLY as u64,
    },
    FlagSpec {
        name: "recall-on-data-access",
        bit: FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS as u64,
    },
    FlagSpec {
        name: "recall-on-open",
        bit: FILE_ATTRIBUTE_RECALL_ON_OPEN as u64,
    },
    FlagSpec {
        name: "reparse-point",
        bit: FILE_ATTRIBUTE_REPARSE_POINT as u64,
    },
    FlagSpec {
        name: "sparse",
        bit: FILE_ATTRIBUTE_SPARSE_FILE as u64,
    },
    FlagSpec {
        name: "system",
        bit: FILE_ATTRIBUTE_SYSTEM as u64,
    },
    FlagSpec {
        name: "temporary",
        bit: FILE_ATTRIBUTE_TEMPORARY as u64,
    },
    FlagSpec {
        name: "unpinned",
        bit: FILE_ATTRIBUTE_UNPINNED as u64,
    },
];

pub(crate) fn active_flag_specs() -> &'static [FlagSpec] {
    FLAG_SPECS
}

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    false
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    false
}
