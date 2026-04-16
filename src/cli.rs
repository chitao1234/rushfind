use crate::parser::parse_command;
use crate::planner::plan_command;
use crate::runner::run_plan;
use std::ffi::OsString;

pub fn run<I>(args: I) -> i32
where
    I: IntoIterator<Item = OsString>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let workers = resolve_worker_count();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();

    match parse_command(&args)
        .and_then(|ast| plan_command(ast, workers))
        .and_then(|plan| run_plan(&plan, &mut stdout, &mut stderr))
    {
        Ok(summary) => {
            if summary.had_runtime_errors || summary.had_action_failures {
                1
            } else {
                0
            }
        }
        Err(error) => {
            eprintln!("findoxide: {}", error);
            error.exit_code
        }
    }
}

fn resolve_worker_count() -> usize {
    std::env::var("FINDOXIDE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
        })
}
