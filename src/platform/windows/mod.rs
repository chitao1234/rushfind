pub(crate) mod accounts;
pub(crate) mod filesystem;
pub(crate) mod locale;

use crate::platform::{PlatformCapabilities, SupportLevel};

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
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

pub(crate) const fn printf_zero_pads_string_fields() -> bool {
    false
}

pub(crate) const fn used_requires_strict_atime_after_ctime() -> bool {
    false
}
