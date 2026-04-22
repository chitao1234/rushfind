use crate::diagnostics::Diagnostic;
pub use crate::platform::filesystem::PlatformPrincipalId as PrincipalId;
use std::ffi::{OsStr, OsString};

pub fn resolve_user_principal(raw: &OsStr) -> Result<PrincipalId, Diagnostic> {
    crate::platform::accounts::backend().resolve_user_principal(raw)
}

pub fn resolve_group_principal(raw: &OsStr) -> Result<PrincipalId, Diagnostic> {
    crate::platform::accounts::backend().resolve_group_principal(raw)
}

pub fn resolve_user_id(raw: &OsStr) -> Result<PrincipalId, Diagnostic> {
    resolve_user_principal(raw)
}

pub fn resolve_group_id(raw: &OsStr) -> Result<PrincipalId, Diagnostic> {
    resolve_group_principal(raw)
}

pub fn user_exists(principal: impl Into<PrincipalId>) -> Result<bool, Diagnostic> {
    let principal = principal.into();
    crate::platform::accounts::backend().user_exists(&principal)
}

pub fn group_exists(principal: impl Into<PrincipalId>) -> Result<bool, Diagnostic> {
    let principal = principal.into();
    crate::platform::accounts::backend().group_exists(&principal)
}

pub fn user_name(principal: impl Into<PrincipalId>) -> Result<Option<OsString>, Diagnostic> {
    let principal = principal.into();
    crate::platform::accounts::backend().user_name(&principal)
}

pub fn group_name(principal: impl Into<PrincipalId>) -> Result<Option<OsString>, Diagnostic> {
    let principal = principal.into();
    crate::platform::accounts::backend().group_name(&principal)
}
