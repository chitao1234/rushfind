pub(crate) mod filesystem;

use crate::platform::{PlatformCapabilities, SupportLevel};

pub(crate) static CAPABILITIES: PlatformCapabilities = PlatformCapabilities::new(
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Exact,
    SupportLevel::Unsupported("named ownership predicates are not implemented on Windows yet"),
    SupportLevel::Unsupported("numeric ownership is not supported on Windows"),
    SupportLevel::Unsupported("access predicates are not implemented on Windows yet"),
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
