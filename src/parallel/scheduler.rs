use crate::parallel::control::GlobalControl;
use crate::parallel::task::ParallelTask;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Scheduler {
    injector: Arc<Injector<ParallelTask>>,
    locals: Vec<Mutex<Option<Worker<ParallelTask>>>>,
    stealers: Vec<Stealer<ParallelTask>>,
    sleep_state: Arc<Mutex<()>>,
    wakeup: Arc<Condvar>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct WorkerHandle {
    local: Worker<ParallelTask>,
    peers: Vec<Stealer<ParallelTask>>,
    injector: Arc<Injector<ParallelTask>>,
    sleep_state: Arc<Mutex<()>>,
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
            sleep_state: Arc::new(Mutex::new(())),
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

    pub(crate) fn push_inject(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
        self.wakeup.notify_one();
    }

    pub(crate) fn push_spill(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
        self.wakeup.notify_one();
    }

    pub(crate) fn push_resume(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
        self.wakeup.notify_one();
    }

    pub(crate) fn notify_sleepers(&self) {
        self.wakeup.notify_all();
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl WorkerHandle {
    pub(crate) fn push_local(&mut self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.local.push(task);
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
        loop {
            if let Some(task) = self.pop_nonblocking() {
                return Some(task);
            }

            if control.workers_should_exit() {
                return None;
            }

            let guard = self
                .sleep_state
                .lock()
                .expect("scheduler sleep mutex poisoned");
            let (_guard, _) = self
                .wakeup
                .wait_timeout(guard, Duration::from_millis(10))
                .expect("scheduler condvar wait failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::task::{ParallelTask, PostOrderResumeTask, PreOrderRootTask};
    use crate::runtime_pipeline::SubtreeBarrierId;
    use std::path::PathBuf;
    use std::thread;

    #[test]
    fn root_and_resume_tasks_both_count_as_outstanding_work() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        scheduler.push_root(
            ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("root"), 0)),
            &control,
        );
        scheduler.push_resume(
            ParallelTask::PostOrderResume(PostOrderResumeTask::for_path(
                PathBuf::from("root"),
                0,
                SubtreeBarrierId(7),
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
        let publisher = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            scheduler_for_publisher.push_spill(
                ParallelTask::PreOrderRoot(PreOrderRootTask::for_path(PathBuf::from("child"), 1)),
                control_for_publisher.as_ref(),
            );
        });

        let task = worker.pop_blocking(control.as_ref());
        publisher.join().unwrap();
        control.task_finished();
        control.task_finished();

        assert!(matches!(task, Some(ParallelTask::PreOrderRoot(_))));
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
                SubtreeBarrierId(9),
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
