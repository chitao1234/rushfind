//! Library unit tests that need no subprocess.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod ctype_class;
mod ctype_profile;
mod ctype_text;
mod path_bytes_core;
mod pattern_lc_ctype;
mod pcre2_regex;
mod regex_lc_ctype;
mod time_tail_helpers;
