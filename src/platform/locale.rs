use crate::diagnostics::Diagnostic;
use crate::messages_locale::{MessagesLocale, prompt_locale_for};
use std::ffi::{CStr, CString};

pub(crate) trait LocaleBackend: Send + Sync {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic>;
    fn affirmative_parser(&self) -> fn(&[u8]) -> bool;
}

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

unsafe extern "C" {
    fn rpmatch(response: *const libc::c_char) -> libc::c_int;
}

impl LocaleBackend for PosixLocaleBackend {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic> {
        unsafe {
            let empty = CString::new("").expect("empty C string must be valid");
            let c_fallback = CString::new("C").expect("C locale name must be valid");

            let mut active = libc::setlocale(libc::LC_MESSAGES, empty.as_ptr());
            if active.is_null() {
                active = libc::setlocale(libc::LC_MESSAGES, c_fallback.as_ptr());
            }
            if active.is_null() {
                return Err(Diagnostic::new("failed to initialize LC_MESSAGES", 1));
            }

            let resolved_name = CStr::from_ptr(active).to_string_lossy().into_owned();
            Ok(MessagesLocale {
                prompt_locale: prompt_locale_for(&resolved_name),
                resolved_name,
            })
        }
    }

    fn affirmative_parser(&self) -> fn(&[u8]) -> bool {
        libc_rpmatch_is_affirmative
    }
}

fn libc_rpmatch_is_affirmative(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.contains(&0) {
        return false;
    }

    let reply = match CString::new(bytes) {
        Ok(reply) => reply,
        Err(_) => return false,
    };

    unsafe { rpmatch(reply.as_ptr()) == 1 }
}
