use crate::diagnostics::Diagnostic;

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
    crate::platform::locale::backend().resolve_messages_locale()
}

#[cfg(test)]
mod tests {
    use super::{MessagesLocale, PromptLocale, prompt_locale_for};
    use crate::platform::locale::{LocaleBackend, resolve_messages_locale_with};

    struct FakeLocaleBackend;

    impl LocaleBackend for FakeLocaleBackend {
        fn resolve_messages_locale(
            &self,
        ) -> Result<MessagesLocale, crate::diagnostics::Diagnostic> {
            Ok(MessagesLocale {
                resolved_name: "fr_CA.UTF-8".into(),
                prompt_locale: PromptLocale::Fr,
            })
        }

        fn affirmative_parser(&self) -> fn(&[u8]) -> bool {
            |_| false
        }
    }

    #[test]
    fn prompt_locale_falls_back_from_full_name_to_language_then_c() {
        assert_eq!(prompt_locale_for("fr_FR.UTF-8"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("fr_CA"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("fr"), PromptLocale::Fr);
        assert_eq!(prompt_locale_for("C.UTF-8"), PromptLocale::C);
        assert_eq!(prompt_locale_for("zz_ZZ"), PromptLocale::C);
    }

    #[test]
    fn prompt_locale_selection_can_be_driven_by_an_injected_backend() {
        let locale = resolve_messages_locale_with(&FakeLocaleBackend).unwrap();
        assert_eq!(locale.resolved_name, "fr_CA.UTF-8");
        assert_eq!(locale.prompt_locale, PromptLocale::Fr);
    }
}
