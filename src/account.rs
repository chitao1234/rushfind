use crate::diagnostics::Diagnostic;
use std::ffi::{OsStr, OsString};

pub fn resolve_user_id(raw: &OsStr) -> Result<u32, Diagnostic> {
    crate::platform::accounts::backend().resolve_user_id(raw)
}

pub fn resolve_group_id(raw: &OsStr) -> Result<u32, Diagnostic> {
    crate::platform::accounts::backend().resolve_group_id(raw)
}

pub fn user_exists(uid: u32) -> Result<bool, Diagnostic> {
    crate::platform::accounts::backend().user_exists(uid)
}

pub fn group_exists(gid: u32) -> Result<bool, Diagnostic> {
    crate::platform::accounts::backend().group_exists(gid)
}

pub fn user_name(uid: u32) -> Result<Option<OsString>, Diagnostic> {
    crate::platform::accounts::backend().user_name(uid)
}

pub fn group_name(gid: u32) -> Result<Option<OsString>, Diagnostic> {
    crate::platform::accounts::backend().group_name(gid)
}
