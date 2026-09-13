//! End-to-end tests that drive the `rfd` binary.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod delete_cli;
mod exec_cli;
mod exit_cli;
mod file_identity;
mod files0_from_cli;
mod follow_mode_cli;
mod follow_mode_loops_cli;
mod follow_mode_loops_parallel;
mod follow_mode_parallel;
mod fprint_cli;
mod loop_detection_cli;
mod ls_cli;
mod ls_escaping;
mod normal_options_cli;
mod ordered_cli;
mod ordered_exit_streaming;
mod parallel_cli;
mod parallel_engine_cli;
mod parallel_postorder_cli;
mod path_bytes_cli;
mod path_name_semantics;
mod printf_cli;
mod quit_cli;
mod sigpipe_cli;
mod version_cli;
