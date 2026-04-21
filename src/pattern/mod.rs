mod ir;
mod owned;
mod parse;
#[cfg(unix)]
mod unix;

use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::sync::Arc;

type PatternMatcher = fn(&OsStr, &OsStr, bool, bool) -> Result<bool, Diagnostic>;

pub fn matches_pattern(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    matches_pattern_with(
        pattern,
        candidate,
        case_insensitive,
        pathname,
        crate::platform::unix::match_pattern,
    )
}

fn matches_pattern_with(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
    matcher: PatternMatcher,
) -> Result<bool, Diagnostic> {
    matcher(pattern, candidate, case_insensitive, pathname)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobCaseMode {
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobSlashMode {
    Literal,
    Pathname,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobLocaleMode {
    CLike,
    RuntimeLocale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobMatchContext {
    locale_mode: GlobLocaleMode,
    unix_fallback_available: bool,
}

impl GlobMatchContext {
    pub fn new(locale_mode: GlobLocaleMode, unix_fallback_available: bool) -> Self {
        Self {
            locale_mode,
            unix_fallback_available,
        }
    }

    pub fn c_locale() -> Self {
        Self::new(GlobLocaleMode::CLike, cfg!(unix))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GlobBackend {
    OwnedOnly,
    OwnedOrUnixFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledGlobInner {
    flag: &'static str,
    original_pattern: Vec<u8>,
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
    backend: GlobBackend,
    program: ir::GlobProgram,
    contains_bracket_expr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGlob {
    inner: Arc<CompiledGlobInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedGlobBackend {
    Owned,
    UnixFallback,
}

impl SelectedGlobBackend {
    pub fn is_owned(self) -> bool {
        matches!(self, Self::Owned)
    }

    pub fn is_unix_fallback(self) -> bool {
        matches!(self, Self::UnixFallback)
    }
}

impl CompiledGlob {
    pub fn compile(
        flag: &'static str,
        pattern: &OsStr,
        case_mode: GlobCaseMode,
        slash_mode: GlobSlashMode,
    ) -> Result<Self, Diagnostic> {
        let original_pattern = pattern.as_encoded_bytes().to_vec();
        let parsed = parse::compile_pattern(flag, &original_pattern, case_mode, slash_mode)?;
        Ok(Self {
            inner: Arc::new(CompiledGlobInner {
                flag,
                original_pattern,
                case_mode,
                slash_mode,
                backend: parsed.backend,
                program: parsed.program,
                contains_bracket_expr: parsed.contains_bracket_expr,
            }),
        })
    }

    pub fn is_match(
        &self,
        candidate: &OsStr,
        context: &GlobMatchContext,
    ) -> Result<bool, Diagnostic> {
        match self.backend_for(context) {
            SelectedGlobBackend::Owned => owned::matches(
                &self.inner.program,
                self.inner.case_mode,
                self.inner.slash_mode,
                candidate.as_encoded_bytes(),
                context,
            ),
            SelectedGlobBackend::UnixFallback => {
                if !context.unix_fallback_available {
                    return Err(Diagnostic::new(
                        "non-C locale glob matching requires a Unix fnmatch fallback on this platform",
                        1,
                    ));
                }
                #[cfg(unix)]
                {
                    unix::fnmatch_fallback(
                        &self.inner.original_pattern,
                        candidate,
                        self.inner.case_mode,
                        self.inner.slash_mode,
                    )
                }
                #[cfg(not(unix))]
                {
                    unreachable!("unix fallback should only be selected on unix targets")
                }
            }
        }
    }

    pub fn backend_for(&self, context: &GlobMatchContext) -> SelectedGlobBackend {
        match self.inner.backend {
            GlobBackend::OwnedOnly => SelectedGlobBackend::Owned,
            GlobBackend::OwnedOrUnixFallback => {
                if context.locale_mode == GlobLocaleMode::CLike {
                    SelectedGlobBackend::Owned
                } else {
                    SelectedGlobBackend::UnixFallback
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn contains_bracket_expr(&self) -> bool {
        self.inner.contains_bracket_expr
    }
}

#[cfg(test)]
mod tests {
    use super::matches_pattern_with;
    use super::{CompiledGlob, GlobCaseMode, GlobMatchContext, GlobSlashMode};
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    fn c_locale_context() -> GlobMatchContext {
        GlobMatchContext::c_locale()
    }

    #[test]
    fn case_insensitive_matching_can_use_a_non_linux_backend() {
        let matched = matches_pattern_with(
            OsStr::new("*.rs"),
            OsStr::new("MAIN.RS"),
            true,
            true,
            |_pattern, _candidate, case_insensitive, pathname| {
                assert!(case_insensitive);
                assert!(pathname);
                Ok(true)
            },
        )
        .unwrap();

        assert!(matched);
    }

    #[test]
    fn basename_star_does_not_treat_slash_as_special() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("*a/b*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(
            glob.is_match(OsStr::new("xa/by"), &c_locale_context())
                .unwrap()
        );
    }

    #[test]
    fn path_star_can_cross_slashes_like_gnu_find() {
        let glob = CompiledGlob::compile(
            "-path",
            OsStr::new("./src/*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(
            glob.is_match(OsStr::new("./src/lib.rs"), &c_locale_context())
                .unwrap()
        );
        assert!(
            glob.is_match(OsStr::new("./src/nested/lib.rs"), &c_locale_context())
                .unwrap()
        );
    }

    #[test]
    fn c_locale_case_insensitive_matching_is_ascii_only() {
        let glob = CompiledGlob::compile(
            "-iname",
            OsStr::new("résumé.*"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(
            glob.is_match(OsStr::new("résumé.MD"), &c_locale_context())
                .unwrap()
        );
        assert!(
            !glob
                .is_match(OsStr::new("RÉSUMÉ.MD"), &c_locale_context())
                .unwrap()
        );
    }

    #[test]
    fn bracket_ranges_follow_c_locale_byte_order() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-C]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(
            glob.is_match(OsStr::new("Bravo"), &c_locale_context())
                .unwrap()
        );
        assert!(
            !glob
                .is_match(OsStr::new("delta"), &c_locale_context())
                .unwrap()
        );
    }

    #[test]
    fn non_utf8_candidates_are_matched_without_lossy_conversion() {
        let candidate = OsString::from_vec(vec![b'f', b'o', b'o', 0xff]);
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("foo*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(
            glob.is_match(candidate.as_os_str(), &c_locale_context())
                .unwrap()
        );
    }
}

#[cfg(test)]
mod backend_selection_tests {
    use super::{CompiledGlob, GlobCaseMode, GlobLocaleMode, GlobMatchContext, GlobSlashMode};
    use std::ffi::OsStr;

    #[test]
    fn glob_runtime_c_locale_always_uses_owned_backend() {
        let glob = CompiledGlob::compile(
            "-iname",
            OsStr::new("*.md"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.backend_for(&GlobMatchContext::c_locale()).is_owned());
    }

    #[test]
    fn glob_runtime_case_insensitive_patterns_request_fallback() {
        let glob = CompiledGlob::compile(
            "-iname",
            OsStr::new("*.md"),
            GlobCaseMode::Insensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        let runtime = GlobMatchContext::new(GlobLocaleMode::RuntimeLocale, true);
        assert!(glob.backend_for(&runtime).is_unix_fallback());
    }

    #[test]
    fn glob_runtime_bracket_patterns_request_fallback() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-Z]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        let runtime = GlobMatchContext::new(GlobLocaleMode::RuntimeLocale, true);
        assert!(glob.backend_for(&runtime).is_unix_fallback());
    }

    #[test]
    fn glob_runtime_compilation_tracks_bracket_expressions() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-Z]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();

        assert!(glob.contains_bracket_expr());
    }
}
