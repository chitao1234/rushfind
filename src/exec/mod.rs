mod batch;
mod child;
mod confirm;
mod delete;
mod ordered;
mod parallel;
mod template;

pub use batch::{BatchLimit, ExecBatchKey, PendingBatch, ReadyBatch};
pub use child::SpillBuffer;
pub use ordered::OrderedActionSink;
pub use parallel::ParallelActionSink;
pub use template::{
    BatchedExecAction, ExecBatchId, ExecSemantics, ExecTemplateSegment, ImmediateExecAction,
    PreparedExecCommand, batched_path_cost, build_batched_argv, build_immediate_command,
    compile_batched_exec, compile_immediate_exec, execdir_cwd, render_immediate_argv,
    render_prompt_argv,
};

pub(crate) use batch::fixed_batch_cost;
#[allow(unused_imports)]
pub(crate) use child::{run_immediate_parallel, run_parallel_ready_batch, run_prepared_inherited};
#[allow(unused_imports)]
pub(crate) use confirm::{ConfirmOutcome, PromptCoordinator};
pub(crate) use delete::delete_path;

#[cfg(all(test, unix))]
mod tests {
    use super::{
        BatchLimit, ExecSemantics, OrderedActionSink, ParallelActionSink, PendingBatch,
        SpillBuffer, build_batched_argv, compile_batched_exec, compile_immediate_exec, delete_path,
        fixed_batch_cost, render_immediate_argv, render_prompt_argv,
    };
    use crate::entry::EntryContext;
    use crate::eval::{ActionSink, EvalContext};
    use crate::follow::FollowMode;
    use crate::planner::RuntimeAction;
    use crossbeam_channel::unbounded;
    use std::io::Write;
    use std::os::unix::fs as unix_fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn batch_sizer_flushes_before_crossing_the_limit() {
        let spec =
            compile_batched_exec(7, ExecSemantics::Normal, &["echo".into(), "{}".into()]).unwrap();
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
        let spec = compile_batched_exec(
            3,
            ExecSemantics::Normal,
            &["printf".into(), "%s\\n".into(), "{}".into()],
        )
        .unwrap();
        let prepared =
            build_batched_argv(&spec, &[PathBuf::from("a"), PathBuf::from("b")]).unwrap();

        assert_eq!(prepared.cwd, None);
        assert_eq!(prepared.argv, vec!["printf", "%s\\n", "a", "b"]);
    }

    #[test]
    fn execdir_rendering_uses_gnu_dot_slash_basename() {
        let spec =
            compile_immediate_exec(ExecSemantics::DirLocal, &["printf".into(), "X{}Y".into()]);

        assert_eq!(
            render_immediate_argv(&spec, Path::new("dir/file.txt")),
            vec!["printf", "X./file.txtY"]
        );
    }

    #[test]
    fn execdir_prompt_rendering_uses_gnu_dot_slash_basename() {
        let spec =
            compile_immediate_exec(ExecSemantics::DirLocal, &["printf".into(), "X{}Y".into()]);

        assert_eq!(
            render_prompt_argv(&spec, Path::new("dir/file.txt")),
            vec!["printf", "X./file.txtY"]
        );
    }

    #[test]
    fn execdir_batched_argv_uses_dot_slash_paths_and_common_directory() {
        let spec = compile_batched_exec(
            11,
            ExecSemantics::DirLocal,
            &["printf".into(), "%s\\n".into(), "{}".into()],
        )
        .unwrap();

        let prepared =
            build_batched_argv(&spec, &[PathBuf::from("dir/a"), PathBuf::from("dir/b")]).unwrap();

        assert_eq!(prepared.cwd, Some(PathBuf::from("dir")));
        assert_eq!(prepared.argv, vec!["printf", "%s\\n", "./a", "./b"]);
    }

    #[test]
    fn execdir_batch_cost_uses_rendered_dot_slash_path_length() {
        let spec = compile_batched_exec(13, ExecSemantics::DirLocal, &["echo".into(), "{}".into()])
            .unwrap();
        let mut batch = PendingBatch::new(
            spec.clone(),
            BatchLimit::for_tests(18),
            fixed_batch_cost(&spec),
        );

        assert_eq!(batch.push("dir/abcdef".as_ref()).unwrap(), None);
        let flushed = batch
            .push("dir/ghijkl".as_ref())
            .unwrap()
            .expect("expected flush");
        assert_eq!(flushed.paths, vec![PathBuf::from("dir/abcdef")]);
    }

    #[test]
    fn execdir_batch_cwd_comes_from_path_parent() {
        use super::ExecBatchKey;

        let spec = compile_batched_exec(21, ExecSemantics::DirLocal, &["echo".into(), "{}".into()])
            .unwrap();

        assert_eq!(
            ExecBatchKey {
                id: spec.id,
                cwd: spec.batch_cwd(Path::new("left/a")),
            },
            ExecBatchKey {
                id: 21,
                cwd: Some(PathBuf::from("left")),
            }
        );
        assert_eq!(
            ExecBatchKey {
                id: spec.id,
                cwd: spec.batch_cwd(Path::new("solo")),
            },
            ExecBatchKey {
                id: 21,
                cwd: Some(PathBuf::from(".")),
            }
        );
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
        let spec =
            compile_batched_exec(7, ExecSemantics::Normal, &["false".into(), "{}".into()]).unwrap();
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
        let spec =
            compile_batched_exec(9, ExecSemantics::Normal, &["false".into(), "{}".into()]).unwrap();
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
