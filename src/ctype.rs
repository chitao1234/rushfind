pub mod case;
pub mod class;
pub mod text;

use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtypeProfile {
    ByteC,
    Encoded(EncodedCtype),
    Unknown(UnknownCtype),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedCtype {
    pub locale_name: String,
    pub codeset_label: String,
    pub encoding_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCtype {
    pub locale_name: String,
    pub codeset_label: Option<String>,
    pub reason: &'static str,
}

impl Default for CtypeProfile {
    fn default() -> Self {
        Self::ByteC
    }
}

impl CtypeProfile {
    pub fn current() -> Self {
        resolve_ctype_profile_from(std::env::vars())
    }

    pub fn is_byte_c(&self) -> bool {
        matches!(self, Self::ByteC)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    pub fn is_encoded_label(&self, expected: &str) -> bool {
        matches!(
            self,
            Self::Encoded(EncodedCtype { encoding_name, .. }) if *encoding_name == expected
        )
    }

    pub fn warning(&self) -> Option<String> {
        match self {
            Self::Unknown(unknown) => Some(format!(
                "unsupported LC_CTYPE encoding{} in `{}`; using byte-oriented C/POSIX matching",
                unknown
                    .codeset_label
                    .as_ref()
                    .map(|label| format!(" `{label}`"))
                    .unwrap_or_default(),
                unknown.locale_name
            )),
            Self::ByteC | Self::Encoded(_) => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn encoding(&self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            Self::Encoded(encoded) => {
                encoding_rs::Encoding::for_label(encoded.codeset_label.as_bytes())
            }
            Self::ByteC | Self::Unknown(_) => None,
        }
    }
}

pub fn resolve_ctype_profile_from<I, K, V>(vars: I) -> CtypeProfile
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut lang = None;
    let mut lc_ctype = None;
    let mut lc_all = None;

    for (key, value) in vars {
        let key = key.as_ref();
        let value = value.as_ref();
        if value.is_empty() {
            continue;
        }
        match key {
            "LANG" => lang = Some(value.to_string()),
            "LC_CTYPE" => lc_ctype = Some(value.to_string()),
            "LC_ALL" => lc_all = Some(value.to_string()),
            _ => {}
        }
    }

    let locale = lc_all
        .or(lc_ctype)
        .or(lang)
        .unwrap_or_else(|| "C".to_string());
    profile_for_locale(&locale)
}

fn profile_for_locale(locale: &str) -> CtypeProfile {
    if matches!(locale, "C" | "POSIX") {
        return CtypeProfile::ByteC;
    }

    let Some(codeset) = extract_codeset(locale) else {
        return CtypeProfile::Unknown(UnknownCtype {
            locale_name: locale.to_string(),
            codeset_label: None,
            reason: "locale has no explicit codeset",
        });
    };

    let normalized = normalize_codeset_label(codeset);
    let Some(encoding) = encoding_rs::Encoding::for_label(normalized.as_bytes()) else {
        return CtypeProfile::Unknown(UnknownCtype {
            locale_name: locale.to_string(),
            codeset_label: Some(codeset.to_string()),
            reason: "codeset is not supported by encoding_rs",
        });
    };

    CtypeProfile::Encoded(EncodedCtype {
        locale_name: locale.to_string(),
        codeset_label: normalized.into_owned(),
        encoding_name: encoding.name(),
    })
}

fn extract_codeset(locale: &str) -> Option<&str> {
    let without_modifier = locale.split_once('@').map_or(locale, |(head, _)| head);
    without_modifier.split_once('.').map(|(_, codeset)| codeset)
}

fn normalize_codeset_label(label: &str) -> Cow<'_, str> {
    let lowered = label.to_ascii_lowercase();
    let compact = lowered
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_' && *ch != '.')
        .collect::<String>();

    match compact.as_str() {
        "utf8" => Cow::Borrowed("utf-8"),
        "eucjp" => Cow::Borrowed("euc-jp"),
        "euckr" => Cow::Borrowed("euc-kr"),
        "sjis" | "shiftjis" | "cp932" | "ms932" => Cow::Borrowed("shift-jis"),
        "koi8r" => Cow::Borrowed("koi8-r"),
        "koi8u" => Cow::Borrowed("koi8-u"),
        "gb18030" => Cow::Borrowed("gb18030"),
        "gbk" | "gb2312" => Cow::Borrowed("gbk"),
        "big5" | "big5hkscs" => Cow::Borrowed("big5"),
        _ => Cow::Owned(lowered),
    }
}
