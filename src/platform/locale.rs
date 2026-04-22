#![cfg_attr(windows, allow(dead_code))]

use crate::diagnostics::Diagnostic;
use crate::messages_locale::{MessagesLocale, prompt_locale_for};
#[cfg(not(windows))]
use std::ffi::CStr;
use std::ffi::CString;

#[cfg(not(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "freebsd",
    target_os = "aix"
)))]
use std::mem::MaybeUninit;

pub(crate) trait LocaleBackend: Send + Sync {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic>;
    fn affirmative_parser(&self) -> fn(&[u8]) -> bool;
}

#[cfg(windows)]
pub(crate) fn backend() -> &'static dyn LocaleBackend {
    &WINDOWS_LOCALE_BACKEND
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

#[cfg(windows)]
static WINDOWS_LOCALE_BACKEND: WindowsLocaleBackend = WindowsLocaleBackend;

#[cfg(windows)]
struct WindowsLocaleBackend;

#[cfg(any(
    all(target_os = "linux", target_env = "gnu"),
    target_os = "freebsd",
    target_os = "aix"
))]
unsafe extern "C" {
    fn rpmatch(response: *const libc::c_char) -> libc::c_int;
}

#[cfg(windows)]
impl LocaleBackend for WindowsLocaleBackend {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic> {
        let resolved_name = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| "C".to_string());
        Ok(MessagesLocale {
            prompt_locale: prompt_locale_for(&resolved_name),
            resolved_name,
        })
    }

    fn affirmative_parser(&self) -> fn(&[u8]) -> bool {
        default_ascii_yes_is_affirmative
    }
}

#[cfg(not(windows))]
impl LocaleBackend for PosixLocaleBackend {
    fn resolve_messages_locale(&self) -> Result<MessagesLocale, Diagnostic> {
        let resolved_name = resolve_locale_category(libc::LC_MESSAGES, "LC_MESSAGES")?;
        Ok(MessagesLocale {
            prompt_locale: prompt_locale_for(&resolved_name),
            resolved_name,
        })
    }

    fn affirmative_parser(&self) -> fn(&[u8]) -> bool {
        libc_affirmative_is_affirmative
    }
}

fn libc_affirmative_is_affirmative(bytes: &[u8]) -> bool {
    let Some(reply) = reply_cstring(bytes) else {
        return false;
    };

    #[cfg(windows)]
    {
        let _ = reply;
        default_ascii_yes_is_affirmative(bytes)
    }

    #[cfg(any(
        all(target_os = "linux", target_env = "gnu"),
        target_os = "freebsd",
        target_os = "aix"
    ))]
    unsafe {
        rpmatch(reply.as_ptr()) == 1
    }

    #[cfg(not(any(
        windows,
        all(target_os = "linux", target_env = "gnu"),
        target_os = "freebsd",
        target_os = "aix"
    )))]
    {
        langinfo_affirmative_is_affirmative(bytes, &reply)
    }
}

fn reply_cstring(bytes: &[u8]) -> Option<CString> {
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }

    CString::new(bytes).ok()
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

#[cfg(not(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "freebsd",
    target_os = "aix"
)))]
fn langinfo_affirmative_is_affirmative(bytes: &[u8], reply: &CString) -> bool {
    if let Some(yesexpr) = langinfo_string(libc::YESEXPR) {
        if let Some(matches) = regexec_matches(&yesexpr, reply) {
            return matches;
        }
    }

    if let Some(yesstr) = langinfo_string(libc::YESSTR) {
        return yesstr_fallback_is_affirmative(bytes, yesstr.to_bytes());
    }

    default_ascii_yes_is_affirmative(bytes)
}

#[cfg(not(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "freebsd",
    target_os = "aix"
)))]
fn langinfo_string(item: libc::nl_item) -> Option<CString> {
    let ptr = unsafe { libc::nl_langinfo(item) };
    if ptr.is_null() {
        return None;
    }

    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    if bytes.is_empty() {
        return None;
    }

    CString::new(bytes).ok()
}

#[cfg(not(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "freebsd",
    target_os = "aix"
)))]
fn regexec_matches(pattern: &CString, reply: &CString) -> Option<bool> {
    let mut regex = MaybeUninit::<libc::regex_t>::zeroed();
    let compile_status = unsafe {
        libc::regcomp(
            regex.as_mut_ptr(),
            pattern.as_ptr(),
            libc::REG_EXTENDED | libc::REG_NOSUB,
        )
    };
    if compile_status != 0 {
        return None;
    }

    let mut regex = unsafe { regex.assume_init() };
    let exec_status = unsafe { libc::regexec(&regex, reply.as_ptr(), 0, std::ptr::null_mut(), 0) };
    unsafe {
        libc::regfree(&mut regex);
    }

    Some(exec_status == 0)
}

#[cfg_attr(
    any(
        all(target_os = "linux", target_env = "gnu"),
        target_os = "freebsd",
        target_os = "aix"
    ),
    allow(dead_code)
)]
fn yesstr_fallback_is_affirmative(bytes: &[u8], yesstr: &[u8]) -> bool {
    if yesstr.is_empty() {
        return default_ascii_yes_is_affirmative(bytes);
    }

    if bytes == yesstr {
        return true;
    }

    if bytes.is_ascii() && yesstr.is_ascii() {
        if bytes.eq_ignore_ascii_case(yesstr) {
            return true;
        }

        return bytes.len() == 1
            && yesstr
                .first()
                .is_some_and(|first| bytes[0].eq_ignore_ascii_case(first));
    }

    false
}

#[cfg_attr(
    any(
        all(target_os = "linux", target_env = "gnu"),
        target_os = "freebsd",
        target_os = "aix"
    ),
    allow(dead_code)
)]
fn default_ascii_yes_is_affirmative(bytes: &[u8]) -> bool {
    bytes.eq_ignore_ascii_case(b"y") || bytes.eq_ignore_ascii_case(b"yes")
}

#[cfg(test)]
mod tests {
    use super::{default_ascii_yes_is_affirmative, yesstr_fallback_is_affirmative};

    #[test]
    fn default_ascii_parser_accepts_y_and_yes() {
        assert!(default_ascii_yes_is_affirmative(b"y"));
        assert!(default_ascii_yes_is_affirmative(b"Y"));
        assert!(default_ascii_yes_is_affirmative(b"yes"));
        assert!(default_ascii_yes_is_affirmative(b"Yes"));
        assert!(!default_ascii_yes_is_affirmative(b"n"));
    }

    #[test]
    fn yesstr_fallback_accepts_full_ascii_word_and_initial_letter() {
        assert!(yesstr_fallback_is_affirmative(b"yes", b"yes"));
        assert!(yesstr_fallback_is_affirmative(b"YES", b"yes"));
        assert!(yesstr_fallback_is_affirmative(b"y", b"yes"));
        assert!(yesstr_fallback_is_affirmative(b"Y", b"yes"));
        assert!(!yesstr_fallback_is_affirmative(b"yeah", b"yes"));
        assert!(!yesstr_fallback_is_affirmative(b"n", b"yes"));
    }
}
