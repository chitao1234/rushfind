pub(crate) mod accounts;
pub(crate) mod capabilities;
pub(crate) mod filesystem;
pub(crate) mod locale;
pub(crate) mod unix;

pub(crate) use capabilities::{PlatformCapabilities, PlatformFeature, SupportLevel};

pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    unix::active_capabilities()
}

pub(crate) fn printf_zero_pads_string_fields() -> bool {
    unix::printf_zero_pads_string_fields()
}

pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    unix::used_requires_strict_atime_after_ctime()
}
