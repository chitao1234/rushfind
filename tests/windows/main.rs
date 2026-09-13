//! Windows-only CLI tests; empty on other platforms.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod windows_exec_cli;
mod windows_flags_cli;
mod windows_ls_cli;
mod windows_owner_sid_cli;
mod windows_path_cli;
mod windows_printf_cli;
mod windows_traversal_cli;
