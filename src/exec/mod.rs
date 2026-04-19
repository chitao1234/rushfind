mod batch;
mod child;
mod delete;
mod ordered;
mod parallel;
mod template;

pub use batch::{BatchLimit, PendingBatch, ReadyBatch};
pub use child::SpillBuffer;
pub use ordered::OrderedActionSink;
pub use parallel::ParallelActionSink;
pub use template::{
    BatchedExecAction, ExecBatchId, ExecTemplateSegment, ImmediateExecAction,
    build_batched_argv, compile_batched_exec, compile_immediate_exec, render_immediate_argv,
};

pub(crate) use batch::fixed_batch_cost;
pub(crate) use child::{run_immediate_parallel, run_parallel_ready_batch};
pub(crate) use delete::delete_path;

#[cfg(test)]
mod tests {
    use super::{
        BatchLimit, OrderedActionSink, ParallelActionSink, PendingBatch, SpillBuffer,
        build_batched_argv, compile_batched_exec, delete_path,
    };
    use crate::entry::EntryContext;
    use crate::eval::{ActionSink, EvalContext};
    use crate::follow::FollowMode;
    use crate::planner::RuntimeAction;
    use crossbeam_channel::unbounded;
    use std::io::Write;
    use std::os::unix::fs as unix_fs;
    use std::path::PathBuf;

    #[test]
    fn batch_sizer_flushes_before_crossing_the_limit() {
        let spec = compile_batched_exec(7, &["echo".into(), "{}".into()]).unwrap();
        let mut batch = PendingBatch::new(spec, BatchLimit::for_tests(16), 0);

        assert_eq!(batch.push("aa".as_ref()).unwrap(), None);
        assert_eq!(batch.push("bbbb".as_ref()).unwrap(), None);
        let flushed = batch
            .push("cccccccc".as_ref())
            .unwrap()
            .expect("expected flush");

        assert_eq!(
            flushed.paths,
            vec![PathBuf::from("aa"), PathBuf::from("bbbb")]
        );
        assert_eq!(batch.paths, vec![PathBuf::from("cccccccc")]);
    }

    #[test]
    fn render_batched_argv_appends_paths_after_the_fixed_prefix() {
        let spec =
            compile_batched_exec(3, &["printf".into(), "%s\\n".into(), "{}".into()]).unwrap();
        let argv = build_batched_argv(&spec, &[PathBuf::from("a"), PathBuf::from("b")]);

        assert_eq!(argv, vec!["printf", "%s\\n", "a", "b"]);
    }

    #[test]
    fn spill_buffer_moves_large_output_to_a_tempfile() {
        let mut buffer = SpillBuffer::new(8).unwrap();
        buffer.write_all(b"12345678").unwrap();
        buffer.write_all(b"abcdef").unwrap();

        assert!(buffer.spilled_path().is_some());
        assert_eq!(buffer.into_bytes().unwrap(), b"12345678abcdef");
    }

    #[test]
    fn delete_path_unlinks_symlinks_without_touching_targets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("target.txt"), "target\n").unwrap();
        unix_fs::symlink("target.txt", root.path().join("link.txt")).unwrap();

        assert!(delete_path(root.path().join("link.txt").as_path()).unwrap());
        assert!(root.path().join("target.txt").exists());
        assert!(!root.path().join("link.txt").exists());
    }

    #[test]
    fn ordered_flush_reports_batched_false_as_action_failure() {
        let spec = compile_batched_exec(7, &["false".into(), "{}".into()]).unwrap();
        let entry = EntryContext::new(PathBuf::from("placeholder"), 0, true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut sink = OrderedActionSink::new(&mut stdout, &mut stderr, &[]).unwrap();
        let context = EvalContext::default();

        let outcome = sink
            .dispatch(
                &RuntimeAction::ExecBatched(spec),
                &entry,
                FollowMode::Physical,
                &context,
            )
            .unwrap();

        assert!(outcome.matched);
        assert!(!outcome.status.had_action_failures());

        let status = sink.flush().unwrap();
        assert!(status.had_action_failures());
    }

    #[test]
    fn parallel_flush_reports_batched_false_as_action_failure() {
        let spec = compile_batched_exec(9, &["false".into(), "{}".into()]).unwrap();
        let entry = EntryContext::new(PathBuf::from("placeholder"), 0, true);
        let (broker, _rx) = unbounded();
        let mut sink = ParallelActionSink::new(broker, 4).unwrap();
        let context = EvalContext::default();

        let outcome = sink
            .dispatch(
                &RuntimeAction::ExecBatched(spec),
                &entry,
                FollowMode::Physical,
                &context,
            )
            .unwrap();

        assert!(outcome.matched);
        assert!(!outcome.status.had_action_failures());

        let status = sink.flush_all().unwrap();
        assert!(status.had_action_failures());
    }

    #[test]
    fn ordered_delete_missing_path_reports_action_failure_status() {
        let missing = EntryContext::new(PathBuf::from("definitely-missing-delete-target"), 0, true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut sink = OrderedActionSink::new(&mut stdout, &mut stderr, &[]).unwrap();
        let context = EvalContext::default();

        let outcome = sink
            .dispatch(
                &RuntimeAction::Delete,
                &missing,
                FollowMode::Physical,
                &context,
            )
            .unwrap();

        assert!(!outcome.matched);
        assert!(outcome.status.had_action_failures());
    }
}
