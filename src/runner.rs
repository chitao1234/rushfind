use crate::diagnostics::Diagnostic;
use crate::eval::EvalContext;
use crate::messages_locale::{MessagesLocale, resolve_messages_locale};
use crate::mounts::MountSnapshot;
use crate::planner::{ExecutionMode, ExecutionPlan, RuntimeExpr, TraversalOrder};
use crate::traversal_control::{TraversalControl, evaluate_for_traversal_with_context};
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    pub had_runtime_errors: bool,
    pub had_action_failures: bool,
}

pub(crate) fn write_startup_warnings<E: Write>(
    warnings: &[String],
    stderr: &mut E,
) -> Result<(), Diagnostic> {
    for warning in warnings {
        writeln!(stderr, "{warning}")
            .map_err(|error| Diagnostic::new(format!("failed to write stderr: {error}"), 1))?;
    }
    Ok(())
}

pub(crate) fn validate_execdir_path_value(path: &OsStr) -> Result<(), Diagnostic> {
    for entry in path.as_bytes().split(|byte| *byte == b':') {
        if entry.is_empty() || entry[0] != b'/' {
            return Err(Diagnostic::new(
                "unsafe PATH for `-execdir`: PATH entries must be absolute and non-empty",
                1,
            ));
        }
    }
    Ok(())
}

fn validate_startup_requirements(plan: &ExecutionPlan) -> Result<(), Diagnostic> {
    if !plan.runtime.execdir_requires_safe_path {
        return Ok(());
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    validate_execdir_path_value(path.as_os_str())
}

pub fn run_plan<W, E>(
    plan: &ExecutionPlan,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<RunSummary, Diagnostic>
where
    W: Write + Send,
    E: Write + Send,
{
    validate_startup_requirements(plan)?;
    let messages_locale = build_messages_locale(plan)?;
    write_startup_warnings(&plan.startup_warnings, stderr)?;
    match plan.mode {
        ExecutionMode::OrderedSingle => {
            crate::ordered::run_ordered_plan(plan, stdout, stderr, messages_locale)
        }
        ExecutionMode::ParallelRelaxed => {
            crate::parallel::run_parallel(plan, stdout, stderr, messages_locale)
        }
    }
}

pub(crate) fn traversal_control_for_entry(
    expr: Option<&RuntimeExpr>,
    follow_mode: crate::follow::FollowMode,
    order: TraversalOrder,
    entry: &crate::entry::EntryContext,
    context: &EvalContext,
) -> Result<TraversalControl, Diagnostic> {
    match expr {
        Some(expr) => evaluate_for_traversal_with_context(expr, entry, follow_mode, order, context),
        None => Ok(TraversalControl::allow()),
    }
}

pub(crate) fn build_eval_context(plan: &ExecutionPlan) -> Result<EvalContext, Diagnostic> {
    build_eval_context_with_loader(plan, MountSnapshot::load_proc_self_mountinfo)
}

pub(crate) fn build_messages_locale(
    plan: &ExecutionPlan,
) -> Result<Option<MessagesLocale>, Diagnostic> {
    build_messages_locale_with(plan, resolve_messages_locale)
}

pub(crate) fn build_messages_locale_with<F>(
    plan: &ExecutionPlan,
    resolve_messages: F,
) -> Result<Option<MessagesLocale>, Diagnostic>
where
    F: FnOnce() -> Result<MessagesLocale, Diagnostic>,
{
    if !plan.runtime.messages_locale_required {
        return Ok(None);
    }

    resolve_messages().map(Some)
}

pub(crate) fn build_eval_context_with_loader<F>(
    plan: &ExecutionPlan,
    load_mount_snapshot: F,
) -> Result<EvalContext, Diagnostic>
where
    F: FnOnce() -> Result<MountSnapshot, Diagnostic>,
{
    if !plan.runtime.mount_snapshot {
        return Ok(EvalContext::with_now(plan.runtime.evaluation_now));
    }

    Ok(EvalContext::with_mount_snapshot_and_now(
        load_mount_snapshot()?,
        plan.runtime.evaluation_now,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        build_eval_context_with_loader, build_messages_locale_with, validate_execdir_path_value,
        write_startup_warnings,
    };
    use crate::follow::FollowMode;
    use crate::ordered::engine::ordered_evaluator_workers;
    use crate::parser::parse_command;
    use crate::planner::{
        ActionProfile, ExecutionMode, ExecutionPlan, OutputAction, RuntimeAction, RuntimeExpr,
        RuntimeRequirements, TraversalOptions, TraversalOrder, plan_command,
    };
    use crate::runtime_policy::RuntimePolicy;
    use crate::time::Timestamp;
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn plan_with_runtime(mount_snapshot: bool) -> ExecutionPlan {
        ExecutionPlan {
            start_paths: vec![PathBuf::from(".")],
            follow_mode: FollowMode::Physical,
            traversal: TraversalOptions {
                min_depth: 0,
                max_depth: None,
                same_file_system: false,
                order: TraversalOrder::PreOrder,
            },
            runtime: RuntimeRequirements {
                mount_snapshot,
                evaluation_now: Timestamp::new(0, 0),
                execdir_requires_safe_path: false,
                messages_locale_required: false,
            },
            startup_warnings: Vec::new(),
            file_outputs: Vec::new(),
            expr: RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
            mode: ExecutionMode::OrderedSingle,
            parallel_policy: None,
            action_profile: ActionProfile::default(),
            runtime_policy: RuntimePolicy::derive(1, TraversalOrder::PreOrder, true),
            traversal_control: None,
        }
    }

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(|item| (*item).into()).collect()
    }

    #[test]
    fn mount_snapshot_loader_is_skipped_when_plan_does_not_require_it() {
        let calls = AtomicUsize::new(0);

        let _ = build_eval_context_with_loader(&plan_with_runtime(false), || {
            calls.fetch_add(1, Ordering::SeqCst);
            panic!("mount snapshot loader should not run");
        })
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mount_snapshot_loader_runs_once_when_plan_requires_it() {
        let calls = AtomicUsize::new(0);

        let _ = build_eval_context_with_loader(&plan_with_runtime(true), || {
            calls.fetch_add(1, Ordering::SeqCst);
            crate::mounts::MountSnapshot::from_mountinfo("1 0 8:1 / / rw - tmpfs tmpfs rw\n")
        })
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn messages_locale_resolver_is_skipped_when_plan_does_not_require_it() {
        let calls = AtomicUsize::new(0);
        let plan = plan_with_runtime(false);

        let locale = build_messages_locale_with(&plan, || {
            calls.fetch_add(1, Ordering::SeqCst);
            panic!("messages locale resolver should not run");
        })
        .unwrap();

        assert!(locale.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ordered_exec_plans_fall_back_to_one_evaluator_for_commit_sensitive_actions() {
        let plan = plan_command(
            parse_command(&argv(&[".", "-exec", "false", "{}", ";"])).unwrap(),
            1,
        )
        .unwrap();

        assert_eq!(ordered_evaluator_workers(&plan), 1);
    }

    #[test]
    fn ordered_output_only_plans_keep_internal_parallelism() {
        let plan = plan_command(parse_command(&argv(&[".", "-print"])).unwrap(), 1).unwrap();

        assert_eq!(
            ordered_evaluator_workers(&plan),
            plan.runtime_policy.evaluation_workers
        );
    }

    #[test]
    fn write_startup_warnings_emits_each_warning_on_its_own_line() {
        let mut stderr = Vec::new();
        write_startup_warnings(
            &[
                "findoxide: warning: unrecognized escape `\\q'".into(),
                "findoxide: warning: unrecognized escape `\\x'".into(),
            ],
            &mut stderr,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "findoxide: warning: unrecognized escape `\\q'\n\
             findoxide: warning: unrecognized escape `\\x'\n"
        );
    }

    #[test]
    fn execdir_path_validation_rejects_relative_and_empty_entries() {
        assert!(validate_execdir_path_value(OsStr::new("/usr/bin:/bin")).is_ok());
        assert!(validate_execdir_path_value(OsStr::new("")).is_err());
        assert!(validate_execdir_path_value(OsStr::new(".:/usr/bin")).is_err());
        assert!(validate_execdir_path_value(OsStr::new("bin:/usr/bin")).is_err());
        assert!(validate_execdir_path_value(OsStr::new(":/usr/bin")).is_err());
        assert!(validate_execdir_path_value(OsStr::new("/usr/bin:")).is_err());
        assert!(validate_execdir_path_value(OsStr::new("/usr/bin::/bin")).is_err());
    }
}
