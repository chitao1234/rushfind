//! Evaluator tests.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod eval_family_a;
mod eval_flags;
mod eval_follow_mode;
mod eval_metadata_ownership;
mod eval_perm;
mod eval_read_only_tail;
mod eval_regex;
mod eval_size_time;
mod eval_subset;
mod eval_symlink_content;
