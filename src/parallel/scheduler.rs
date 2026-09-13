use crate::parallel::control::GlobalControl;
use crate::parallel::task::ParallelTask;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Backstop for a missed wake-up. The generation counter makes waiting exact,
/// so this only bounds the damage of a protocol bug: a sleeper wakes up late
/// instead of never.
const SLEEP_BACKSTOP: Duration = Duration::from_millis(100);

/// Sleepers observe this counter to decide whether anything changed while they
/// were running. Every state change that can make work available bumps it, so
/// a worker that has already checked for work cannot miss the wake-up.
#[derive(Debug, Default)]
struct SleepState {
    generation: u64,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Scheduler {
    injector: Arc<Injector<ParallelTask>>,
    locals: Vec<Mutex<Option<Worker<ParallelTask>>>>,
    stealers: Vec<Stealer<ParallelTask>>,
    sleep_state: Arc<Mutex<SleepState>>,
    wakeup: Arc<Condvar>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct WorkerHandle {
    local: Worker<ParallelTask>,
    peers: Vec<Stealer<ParallelTask>>,
    injector: Arc<Injector<ParallelTask>>,
    sleep_state: Arc<Mutex<SleepState>>,
    wakeup: Arc<Condvar>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl Scheduler {
    pub(crate) fn new(worker_count: usize) -> Self {
        let mut locals = Vec::with_capacity(worker_count);
        let mut stealers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let worker = Worker::new_fifo();
            stealers.push(worker.stealer());
            locals.push(Mutex::new(Some(worker)));
        }

        Self {
            injector: Arc::new(Injector::new()),
            locals,
            stealers,
            sleep_state: Arc::new(Mutex::new(SleepState::default())),
            wakeup: Arc::new(Condvar::new()),
        }
    }

    pub(crate) fn worker_handle(&self, index: usize) -> WorkerHandle {
        let local = self.locals[index]
            .lock()
            .expect("scheduler worker slot lock poisoned")
            .take()
            .expect("worker handle requested more than once");
        let peers = self
            .stealers
            .iter()
            .enumerate()
            .filter(|(peer_index, _)| *peer_index != index)
            .map(|(_, stealer)| stealer.clone())
            .collect();

        WorkerHandle {
            local,
            peers,
            injector: self.injector.clone(),
            sleep_state: self.sleep_state.clone(),
            wakeup: self.wakeup.clone(),
        }
    }

    pub(crate) fn push_root(&self, task: ParallelTask, control: &GlobalControl) {
        self.push_inject(task, control);
    }

    fn push_inject(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
        wake_sleepers(&self.sleep_state, &self.wakeup);
    }

    /// Wakes every sleeper so that it re-checks for work and for the exit
    /// condition. Callers use it when outstanding work reaches zero.
    pub(crate) fn notify_sleepers(&self) {
        wake_sleepers(&self.sleep_state, &self.wakeup);
    }
}

/// Bumps the generation and wakes every sleeper. A single wake-up is not
/// enough: the woken worker may consume one task and return to work while
/// other queued tasks stay unclaimed, so every sleeper has to re-check.
fn wake_sleepers(sleep_state: &Mutex<SleepState>, wakeup: &Condvar) {
    sleep_state
        .lock()
        .expect("scheduler sleep mutex poisoned")
        .generation += 1;
    wakeup.notify_all();
}

#[cfg_attr(not(test), allow(dead_code))]
impl WorkerHandle {
    pub(crate) fn push_local(&mut self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.local.push(task);
        // A peer may steal this task, so sleeping workers have to look.
        wake_sleepers(&self.sleep_state, &self.wakeup);
    }

    pub(crate) fn pop(&mut self) -> Option<ParallelTask> {
        self.pop_nonblocking()
    }

    fn pop_nonblocking(&mut self) -> Option<ParallelTask> {
        if let Some(task) = self.local.pop() {
            return Some(task);
        }

        if let Steal::Success(task) = self.injector.steal_batch_and_pop(&self.local) {
            return Some(task);
        }

        for peer in &self.peers {
            if let Steal::Success(task) = peer.steal() {
                return Some(task);
            }
        }

        None
    }

    pub(crate) fn pop_blocking(&mut self, control: &GlobalControl) -> Option<ParallelTask> {
        let mut seen = self
            .sleep_state
            .lock()
            .expect("scheduler sleep mutex poisoned")
            .generation;

        loop {
            if let Some(task) = self.pop_nonblocking() {
                return Some(task);
            }
            if control.workers_should_exit() {
                return None;
            }

            let state = self
                .sleep_state
                .lock()
                .expect("scheduler sleep mutex poisoned");
            if state.generation != seen {
                // Something changed while we were looking for work; re-check
                // before sleeping again.
                seen = state.generation;
                continue;
            }

            let (state, _) = self
                .wakeup
                .wait_timeout(state, SLEEP_BACKSTOP)
                .expect("scheduler condvar wait failed");
            seen = state.generation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::task::{
        ParallelTask, PostOrderResumeTask, PreOrderRootTask, SiblingChunkTask,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::Instant;

    #[test]
    fn root_and_resume_tasks_both_count_as_outstanding_work() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root"), 0)),
            &control,
        );
        scheduler.push_inject(
            ParallelTask::PostOrderResume(PostOrderResumeTask::for_path(
                PathBuf::from("root"),
                0,
                None,
            )),
            &control,
        );
        assert_eq!(control.outstanding_tasks(), 2);
    }

    #[test]
    fn blocking_pop_wakes_after_spill_task_arrives() {
        let scheduler = Arc::new(Scheduler::new(1));
        let control = Arc::new(GlobalControl::new());
        let mut worker = scheduler.worker_handle(0);
        control.task_spawned();

        let scheduler_for_publisher = scheduler.clone();
        let control_for_publisher = control.clone();
        let delay = Duration::from_millis(25);
        // Stamped just before the push, so the assertion times the wake-up
        // rather than however late a loaded machine ran the publisher.
        let started = Instant::now();
        let pushed_at = Arc::new(AtomicU64::new(0));
        let pushed_at_for_publisher = pushed_at.clone();
        let publisher = thread::spawn(move || {
            thread::sleep(delay);
            pushed_at_for_publisher.store(started.elapsed().as_nanos() as u64, Ordering::SeqCst);
            scheduler_for_publisher.push_inject(
                ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("child"), 1)),
                control_for_publisher.as_ref(),
            );
        });

        let task = worker.pop_blocking(control.as_ref());
        let woke_at = started.elapsed();
        publisher.join().unwrap();
        control.task_finished();
        control.task_finished();

        assert!(matches!(task, Some(ParallelTask::PreOrderRoot(_))));
        // The sleeper has to be woken by the enqueue, not by the backstop that
        // guards against a missed wake-up, so bound the wait that follows the
        // push rather than the whole test.
        let woke_after =
            woke_at.saturating_sub(Duration::from_nanos(pushed_at.load(Ordering::SeqCst)));
        assert!(
            woke_after < SLEEP_BACKSTOP / 2,
            "woke {woke_after:?} after the enqueue, which suggests the push did not wake the sleeper"
        );
    }

    #[test]
    fn locally_published_task_counts_as_outstanding_work() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        let mut worker = scheduler.worker_handle(0);

        worker.push_local(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("child"), 1)),
            &control,
        );

        assert_eq!(control.outstanding_tasks(), 1);
    }

    #[test]
    fn worker_pops_locally_published_task_before_injected_work() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        let mut worker = scheduler.worker_handle(0);

        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root"), 0)),
            &control,
        );
        worker.push_local(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("child"), 1)),
            &control,
        );

        let first = worker.pop();
        let second = worker.pop();

        assert!(matches!(
            first,
            Some(ParallelTask::PreOrderRoot(task)) if task.pending.path.ends_with("child")
        ));
        assert!(matches!(
            second,
            Some(ParallelTask::PreOrderRoot(task)) if task.pending.path.ends_with("root")
        ));
    }

    #[test]
    fn peer_can_steal_locally_published_task() {
        let scheduler = Scheduler::new(2);
        let control = GlobalControl::new();
        let mut owner = scheduler.worker_handle(0);
        let mut thief = scheduler.worker_handle(1);

        owner.push_local(
            ParallelTask::PostOrderResume(PostOrderResumeTask::for_path(
                PathBuf::from("dir"),
                1,
                None,
            )),
            &control,
        );

        let stolen = thief.pop();

        assert!(matches!(
            stolen,
            Some(ParallelTask::PostOrderResume(task)) if task.entry.path.ends_with("dir")
        ));
    }

    #[test]
    fn peer_can_steal_locally_published_sibling_chunk() {
        let scheduler = Scheduler::new(2);
        let control = GlobalControl::new();
        let mut owner = scheduler.worker_handle(0);
        let mut thief = scheduler.worker_handle(1);

        owner.push_local(
            ParallelTask::SiblingChunk(SiblingChunkTask {
                pending: vec![PreOrderRootTask::for_path(PathBuf::from("dir"), 1).pending],
                completion_barrier: None,
            }),
            &control,
        );

        let stolen = thief.pop();

        assert!(matches!(
            stolen,
            Some(ParallelTask::SiblingChunk(task)) if task.pending[0].path.ends_with("dir")
        ));
    }

    #[test]
    fn quit_wakes_sleeping_workers_and_returns_none_when_idle() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        let mut worker = scheduler.worker_handle(0);

        control.request_quit();
        scheduler.notify_sleepers();

        assert!(worker.pop_blocking(&control).is_none());
    }

    #[test]
    fn steal_path_drains_injected_work_after_local_queue_empties() {
        let scheduler = Scheduler::new(2);
        let control = GlobalControl::new();
        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root-a"), 0)),
            &control,
        );
        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root-b"), 0)),
            &control,
        );

        let mut worker0 = scheduler.worker_handle(0);
        let mut worker1 = scheduler.worker_handle(1);

        assert!(matches!(worker0.pop(), Some(ParallelTask::PreOrderRoot(_))));
        assert!(matches!(worker1.pop(), Some(ParallelTask::PreOrderRoot(_))));
    }

    #[test]
    fn outstanding_task_count_hits_zero_only_after_finish() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root"), 0)),
            &control,
        );

        assert_eq!(control.outstanding_tasks(), 1);
        control.task_finished();
        assert_eq!(control.outstanding_tasks(), 0);
    }
}
