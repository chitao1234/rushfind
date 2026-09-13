//! Command-line parser tests.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod parser_access_predicates;
mod parser_bsd_options;
mod parser_delete_depth;
mod parser_exec;
mod parser_expression_operators;
mod parser_family_a;
mod parser_flags;
mod parser_follow_mode;
mod parser_fprint;
mod parser_fstype;
mod parser_ls;
mod parser_metadata_ownership;
mod parser_normal_options;
mod parser_perm;
mod parser_printf;
mod parser_quit;
mod parser_read_only_tail;
mod parser_regex;
mod parser_size_time;
mod parser_subset;
mod parser_symlink_content;
mod parser_traversal_controls;
mod parser_type_lists;
mod parser_version;
