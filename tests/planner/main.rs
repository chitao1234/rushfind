//! Planner and optimizer tests.
//!
//! Grouped into one test target per family so that editing `src/`
//! relinks seven executables instead of one hundred. See
//! `docs/development.md`.

#[path = "../support/mod.rs"]
mod support;

mod planner_access_predicates;
mod planner_behavior;
mod planner_delete;
mod planner_exec;
mod planner_execution_mode;
mod planner_expression_operators;
mod planner_family_a;
mod planner_flags;
mod planner_follow_mode;
mod planner_fprint;
mod planner_fstype;
mod planner_ls;
mod planner_metadata_ownership;
mod planner_normal_options;
mod planner_optimizer_level;
mod planner_performance_substrate;
mod planner_perm;
mod planner_printf;
mod planner_quit;
mod planner_read_only_tail;
mod planner_regex;
mod planner_size_time;
mod planner_traversal_controls;
