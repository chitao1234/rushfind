use crate::ast::{
    Action, CommandAst, CompatibilityOptions, DebugOption, Expr, Files0From, GlobalOption,
    WarningMode,
};
use crate::diagnostics::{Diagnostic, failed_to_write};
use crate::parser::parse_command;
use crate::planner::plan_command;
use crate::runner::run_plan;
use crate::version::{write_debug_help, write_help, write_version_line};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

pub fn run<I>(args: I) -> i32
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let workers = resolve_worker_count().count;
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();

    match parse_command(&args).and_then(|ast| {
        if ast
            .global_options
            .iter()
            .any(|option| matches!(option, GlobalOption::Help))
        {
            write_help(&mut stdout)?;
            return Ok(0);
        }

        if ast
            .global_options
            .iter()
            .any(|option| matches!(option, GlobalOption::Version))
        {
            write_version_line(&mut stdout)?;
            return Ok(0);
        }

        if ast
            .compatibility_options
            .debug_options
            .contains(&DebugOption::Help)
        {
            write_debug_help(&mut stdout)?;
            return Ok(0);
        }

        write_debug_diagnostics(&ast.compatibility_options, &mut stderr)?;

        let ast = prepare_command(ast)?;
        let plan = plan_command(ast, workers)?;
        let summary = run_plan(&plan, &mut stdout, &mut stderr)?;
        Ok(
            if summary.had_runtime_errors || summary.had_action_failures {
                1
            } else {
                0
            },
        )
    }) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("rfd: {}", error);
            error.exit_code
        }
    }
}

fn prepare_command(mut ast: CommandAst) -> Result<CommandAst, Diagnostic> {
    let Some(files0_from) = ast.compatibility_options.files0_from.clone() else {
        return Ok(ast);
    };

    if ast.start_paths_explicit {
        return Err(Diagnostic::parse(
            "extra operand with -files0-from: command-line paths cannot be combined with -files0-from",
        ));
    }

    if matches!(files0_from, Files0From::Stdin) && expression_contains_prompt_action(&ast.expr) {
        return Err(Diagnostic::parse(
            "option -files0-from reading from standard input cannot be combined with -ok or -okdir",
        ));
    }

    ast.start_paths = read_files0_from(files0_from)?;
    ast.start_paths_explicit = true;
    Ok(ast)
}

fn read_files0_from(source: Files0From) -> Result<Vec<PathBuf>, Diagnostic> {
    let bytes = match source {
        Files0From::Path(path) => std::fs::read(&path).map_err(|error| {
            Diagnostic::parse(format!(
                "failed to read -files0-from `{}`: {error}",
                path.display()
            ))
        })?,
        Files0From::Stdin => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).map_err(|error| {
                Diagnostic::parse(format!(
                    "failed to read -files0-from standard input: {error}"
                ))
            })?;
            bytes
        }
    };

    parse_nul_paths(&bytes)
}

fn parse_nul_paths(bytes: &[u8]) -> Result<Vec<PathBuf>, Diagnostic> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != 0 {
            continue;
        }

        let component = &bytes[start..index];
        if component.is_empty() {
            return Err(Diagnostic::parse(format!(
                "-files0-from:{}: invalid zero-length file name",
                paths.len() + 1
            )));
        }
        paths.push(path_from_nul_component(component)?);
        start = index + 1;
    }

    if start < bytes.len() {
        paths.push(path_from_nul_component(&bytes[start..])?);
    }

    Ok(paths)
}

#[cfg(unix)]
fn path_from_nul_component(component: &[u8]) -> Result<PathBuf, Diagnostic> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(component.to_vec())))
}

#[cfg(not(unix))]
fn path_from_nul_component(component: &[u8]) -> Result<PathBuf, Diagnostic> {
    let value = std::str::from_utf8(component).map_err(|_| {
        Diagnostic::parse("-files0-from contains a path that is not valid UTF-8 on this platform")
    })?;
    Ok(PathBuf::from(value))
}

fn expression_contains_prompt_action(expr: &Expr) -> bool {
    match expr {
        Expr::And(items) | Expr::Sequence(items) => {
            items.iter().any(expression_contains_prompt_action)
        }
        Expr::Or(left, right) => {
            expression_contains_prompt_action(left) || expression_contains_prompt_action(right)
        }
        Expr::Not(inner) => expression_contains_prompt_action(inner),
        Expr::Action(Action::Ok { .. } | Action::OkDir { .. }) => true,
        Expr::Predicate(_) | Expr::Action(_) => false,
    }
}

fn write_debug_diagnostics<W: Write>(
    options: &CompatibilityOptions,
    stderr: &mut W,
) -> Result<(), Diagnostic> {
    for option in options.debug_options.iter().copied() {
        if option == DebugOption::Help {
            continue;
        }
        writeln!(
            stderr,
            "rfd: debug[{}]: requested; detailed GNU find tracing is not implemented",
            option.name()
        )
        .map_err(|error| failed_to_write("stderr", error))?;
    }

    if options.warning_mode == WarningMode::Warn {
        for raw in &options.unknown_debug_options {
            writeln!(
                stderr,
                "rfd: warning: unknown debug option `{}` ignored",
                raw.to_string_lossy()
            )
            .map_err(|error| failed_to_write("stderr", error))?;
        }
    }

    Ok(())
}

const DEFAULT_PARALLEL_WORKER_CAP: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedWorkerCount {
    count: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    source: WorkerCountSource,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerCountSource {
    Explicit,
    Default,
}

fn resolve_worker_count() -> ResolvedWorkerCount {
    let env_value = std::env::var("RUSHFIND_WORKERS").ok();
    let host_parallelism = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    resolve_worker_count_from(env_value.as_deref(), host_parallelism)
}

fn resolve_worker_count_from(
    env_value: Option<&str>,
    host_parallelism: usize,
) -> ResolvedWorkerCount {
    if let Some(count) = env_value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
    {
        return ResolvedWorkerCount {
            count,
            source: WorkerCountSource::Explicit,
        };
    }

    ResolvedWorkerCount {
        count: host_parallelism.clamp(1, DEFAULT_PARALLEL_WORKER_CAP),
        source: WorkerCountSource::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PARALLEL_WORKER_CAP, WorkerCountSource, resolve_worker_count_from};

    #[test]
    fn explicit_positive_worker_count_is_honored_exactly() {
        let resolved = resolve_worker_count_from(Some("8"), 64);

        assert_eq!(resolved.count, 8);
        assert_eq!(resolved.source, WorkerCountSource::Explicit);
    }

    #[test]
    fn implicit_worker_count_is_capped_by_default_policy() {
        let resolved = resolve_worker_count_from(None, DEFAULT_PARALLEL_WORKER_CAP + 8);

        assert_eq!(resolved.count, DEFAULT_PARALLEL_WORKER_CAP);
        assert_eq!(resolved.source, WorkerCountSource::Default);
    }

    #[test]
    fn implicit_worker_count_keeps_small_hosts_small() {
        let resolved = resolve_worker_count_from(None, 2);

        assert_eq!(resolved.count, 2);
        assert_eq!(resolved.source, WorkerCountSource::Default);
    }

    #[test]
    fn invalid_worker_env_falls_back_to_capped_default() {
        for raw in ["", "0", "-3", "many"] {
            let resolved = resolve_worker_count_from(Some(raw), DEFAULT_PARALLEL_WORKER_CAP + 8);

            assert_eq!(resolved.count, DEFAULT_PARALLEL_WORKER_CAP, "{raw}");
            assert_eq!(resolved.source, WorkerCountSource::Default, "{raw}");
        }
    }

    #[test]
    fn implicit_worker_count_never_drops_below_one() {
        let resolved = resolve_worker_count_from(None, 0);

        assert_eq!(resolved.count, 1);
        assert_eq!(resolved.source, WorkerCountSource::Default);
    }
}
