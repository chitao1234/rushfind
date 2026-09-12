use crate::diagnostics::Diagnostic;
use crate::messages_locale::{MessagesLocale, prompt_locale_for};
use crate::platform::locale::LocaleBackend;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

static WINDOWS_LOCALE_BACKEND: WindowsLocaleBackend = WindowsLocaleBackend;
const LOCALE_NAME_MAX_LENGTH: usize = 85;

pub(crate) fn backend() -> &'static dyn LocaleBackend {
    &WINDOWS_LOCALE_BACKEND
}

struct WindowsLocaleBackend;

impl LocaleBackend for WindowsLocaleBackend {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic> {
        let resolved_name = resolve_windows_messages_locale();
        Ok(MessagesLocale {
            prompt_locale: prompt_locale_for(&resolved_name),
            resolved_name,
        })
    }
}

fn resolve_windows_messages_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(native_windows_locale_name)
}

fn native_windows_locale_name() -> String {
    let mut buffer = vec![0u16; LOCALE_NAME_MAX_LENGTH];
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 0 {
        return "C".to_string();
    }

    let locale = OsString::from_wide(&buffer[..(written as usize).saturating_sub(1)])
        .to_string_lossy()
        .into_owned();
    locale.replace('-', "_")
}
