pub(crate) mod accounts;
pub(crate) mod capabilities;
pub(crate) mod filesystem;
pub(crate) mod locale;
pub(crate) mod path;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub(crate) use capabilities::{PlatformCapabilities, PlatformFeature, SupportLevel};

#[cfg(unix)]
pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    unix::active_capabilities()
}

#[cfg(windows)]
pub(crate) fn active_capabilities() -> &'static PlatformCapabilities {
    &windows::CAPABILITIES
}

#[cfg(unix)]
pub(crate) fn active_flag_specs() -> &'static [crate::file_flags::FlagSpec] {
    unix::active_flag_specs()
}

#[cfg(windows)]
pub(crate) fn active_flag_specs() -> &'static [crate::file_flags::FlagSpec] {
    windows::active_flag_specs()
}

#[cfg(unix)]
pub(crate) fn printf_zero_pads_string_fields() -> bool {
    unix::printf_zero_pads_string_fields()
}

#[cfg(windows)]
pub(crate) fn printf_zero_pads_string_fields() -> bool {
    windows::printf_zero_pads_string_fields()
}

#[cfg(unix)]
pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    unix::used_requires_strict_atime_after_ctime()
}

#[cfg(windows)]
pub(crate) fn used_requires_strict_atime_after_ctime() -> bool {
    windows::used_requires_strict_atime_after_ctime()
}
