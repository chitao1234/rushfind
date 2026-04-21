mod ir;
mod owned;
mod parse;

use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;
use std::sync::Arc;

pub fn matches_pattern(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    CompiledGlob::compile(
        if pathname {
            if case_insensitive { "-ipath" } else { "-path" }
        } else if case_insensitive {
            "-iname"
        } else {
            "-name"
        },
        pattern,
        if case_insensitive {
            GlobCaseMode::Insensitive
        } else {
            GlobCaseMode::Sensitive
        },
        if pathname {
            GlobSlashMode::Pathname
        } else {
            GlobSlashMode::Literal
        },
    )
    .and_then(|glob| glob.is_match(candidate, &GlobMatchContext::c_locale()))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlobMatchContext;

impl GlobMatchContext {
    pub fn c_locale() -> Self {
        Self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledGlobInner {
    case_mode: GlobCaseMode,
    slash_mode: GlobSlashMode,
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
}

impl SelectedGlobBackend {
    pub fn is_owned(self) -> bool {
        matches!(self, Self::Owned)
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
                case_mode,
                slash_mode,
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
        owned::matches(
            &self.inner.program,
            self.inner.case_mode,
            self.inner.slash_mode,
            candidate.as_encoded_bytes(),
            context,
        )
    }

    pub fn backend_for(&self, _context: &GlobMatchContext) -> SelectedGlobBackend {
        SelectedGlobBackend::Owned
    }

    #[cfg(test)]
    pub(crate) fn contains_bracket_expr(&self) -> bool {
        self.inner.contains_bracket_expr
    }
}

#[cfg(test)]
mod tests {
    use super::{CompiledGlob, GlobCaseMode, GlobMatchContext, GlobSlashMode, matches_pattern};
    use std::ffi::{OsStr, OsString};
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;

    fn c_locale_context() -> GlobMatchContext {
        GlobMatchContext::c_locale()
    }

    #[test]
    fn matches_pattern_uses_owned_case_insensitive_semantics() {
        let matched = matches_pattern(
            OsStr::new("*.rs"),
            OsStr::new("MAIN.RS"),
            true,
            true,
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
    use super::{CompiledGlob, GlobCaseMode, GlobMatchContext, GlobSlashMode};
    use std::ffi::OsStr;

    #[test]
    fn glob_patterns_always_use_owned_backend_in_c_locale() {
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
    fn glob_case_insensitive_patterns_stay_owned() {
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
    fn glob_bracket_patterns_stay_owned() {
        let glob = CompiledGlob::compile(
            "-name",
            OsStr::new("[A-Z]*"),
            GlobCaseMode::Sensitive,
            GlobSlashMode::Literal,
        )
        .unwrap();
        assert!(glob.backend_for(&GlobMatchContext::c_locale()).is_owned());
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
