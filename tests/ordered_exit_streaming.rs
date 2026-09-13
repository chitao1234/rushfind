mod support;

use support::{path_arg, rushfind_command_with_workers};
use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;

fn wide_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    // Enough entries that parallel evaluators run ahead of the collector.
    for index in 0..200 {
        fs::write(root.path().join(format!("f{index:03}")), "content\n").unwrap();
    }
    root
}

fn run_exit_plan(root: &std::path::Path, workers: usize) -> (Vec<u8>, i32) {
    let output = rushfind_command_with_workers(workers)
        .args([
            path_arg(root),
            OsString::from("-name"),
            OsString::from("f000"),
            OsString::from("-print"),
            OsString::from("-exit"),
            OsString::from("7"),
        ])
        .output()
        .unwrap();

    (output.stdout, output.status.code().unwrap())
}

/// `-exit` stops the traversal where it is reached. Everything the pipeline had
/// already buffered for later entries must stay undispatched, which is easy to
/// get wrong once evaluators run ahead of the collector.
#[test]
fn exit_stops_the_stream_without_emitting_buffered_entries() {
    let root = wide_tree();

    let (baseline, baseline_code) = run_exit_plan(root.path(), 1);
    assert_eq!(baseline_code, 7);
    assert_eq!(
        baseline.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "-exit must emit only the entry it stopped on: {:?}",
        String::from_utf8_lossy(&baseline)
    );

    for _ in 0..12 {
        for workers in [2, 4] {
            let (stdout, code) = run_exit_plan(root.path(), workers);
            assert_eq!(code, 7, "{workers} worker(s) must honour the exit status");
            assert_eq!(
                stdout,
                baseline,
                "{workers} worker(s) emitted entries after -exit: {:?}",
                String::from_utf8_lossy(&stdout)
            );
        }
    }
}
