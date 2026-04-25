use crate::ast::GlobalOption;
use crate::parser::parse_command;
use crate::planner::plan_command;
use crate::runner::run_plan;
use crate::version::write_version_line;
use std::ffi::OsString;

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
            .any(|option| matches!(option, GlobalOption::Version))
        {
            write_version_line(&mut stdout)?;
            return Ok(0);
        }

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
        count: host_parallelism.max(1).min(DEFAULT_PARALLEL_WORKER_CAP),
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
