#![cfg_attr(windows, allow(dead_code))]

use crate::diagnostics::Diagnostic;
use crate::messages_locale::MessagesLocale;
#[cfg(not(windows))]
use crate::messages_locale::prompt_locale_for;
#[cfg(not(windows))]
use std::ffi::CStr;
use std::ffi::CString;

pub(crate) trait LocaleBackend: Send + Sync {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic>;
}

#[cfg(windows)]
pub(crate) fn backend() -> &'static dyn LocaleBackend {
    crate::platform::windows::locale::backend()
}

#[cfg(not(windows))]
pub(crate) fn backend() -> &'static dyn LocaleBackend {
    &POSIX_LOCALE_BACKEND
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_messages_locale_with(
    backend: &dyn LocaleBackend,
) -> Result<MessagesLocale, Diagnostic> {
    backend.resolve_messages_locale()
}

static POSIX_LOCALE_BACKEND: PosixLocaleBackend = PosixLocaleBackend;

struct PosixLocaleBackend;

#[cfg(not(windows))]
impl LocaleBackend for PosixLocaleBackend {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic> {
        let resolved_name = resolve_locale_category(libc::LC_MESSAGES, "LC_MESSAGES")?;
        Ok(MessagesLocale {
            prompt_locale: prompt_locale_for(&resolved_name),
            resolved_name,
        })
    }
}

#[cfg(not(windows))]
fn resolve_locale_category(category: libc::c_int, label: &str) -> Result<String, Diagnostic> {
    unsafe {
        let empty = CString::new("").expect("empty C string must be valid");
        let c_fallback = CString::new("C").expect("C locale name must be valid");

        let mut active = libc::setlocale(category, empty.as_ptr());
        if active.is_null() {
            active = libc::setlocale(category, c_fallback.as_ptr());
        }
        if active.is_null() {
            return Err(Diagnostic::new(format!("failed to initialize {label}"), 1));
        }

        Ok(CStr::from_ptr(active).to_string_lossy().into_owned())
    }
}
