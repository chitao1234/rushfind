//! Differential tests against GNU find.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod gnu_differential;
mod gnu_differential_lc_ctype;
mod gnu_differential_path_bytes;
mod gnu_differential_regex_bytes;
