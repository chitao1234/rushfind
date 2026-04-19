use crate::diagnostics::Diagnostic;
use std::ffi::{CStr, CString};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptLocale {
    C,
    Fr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptFragments {
    pub(crate) prefix: &'static [u8],
    pub(crate) ellipsis: &'static [u8],
    pub(crate) suffix: &'static [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessagesLocale {
    pub(crate) resolved_name: String,
    pub(crate) prompt_locale: PromptLocale,
}

fn locale_candidates(name: &str) -> [&str; 3] {
    let exact = name;
    let no_codeset = exact
        .split_once('.')
        .map_or(exact, |(head, _tail)| head)
        .split_once('@')
        .map_or(
            exact.split_once('.').map_or(exact, |(head, _tail)| head),
            |(head, _tail)| head,
        );
    let language = no_codeset
        .split_once('_')
        .map_or(no_codeset, |(head, _tail)| head);
    [exact, no_codeset, language]
}

pub(crate) fn prompt_locale_for(name: &str) -> PromptLocale {
    for candidate in locale_candidates(name) {
        if candidate == "fr" {
            return PromptLocale::Fr;
        }
        if matches!(candidate, "C" | "POSIX") {
            return PromptLocale::C;
        }
    }

    PromptLocale::C
}

impl MessagesLocale {
    pub(crate) fn prompt_fragments(&self) -> PromptFragments {
        match self.prompt_locale {
            PromptLocale::C => PromptFragments {
                prefix: b"< ",
                ellipsis: b" ...",
                suffix: b" > ? ",
            },
            PromptLocale::Fr => PromptFragments {
                prefix: b"< ",
                ellipsis: b" ...",
                suffix: b" > ? ",
            },
        }
    }
}

pub(crate) fn resolve_messages_locale() -> Result<MessagesLocale, Diagnostic> {
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

#[cfg(test)]
mod tests {
    use super::{PromptLocale, prompt_locale_for};

    #[test]
    fn prompt_locale_falls_back_from_full_name_to_language_then_c() {
        assert_eq!(prompt_locale_for("fr_FR.UTF-8"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("fr_CA"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("fr"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("C.UTF-8"), PromptLocale::C);
        assert_eq!(prompt_locale_for("zz_ZZ"), PromptLocale::C);
    }
}
