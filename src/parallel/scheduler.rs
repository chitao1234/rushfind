use crate::parallel::control::GlobalControl;
use crate::parallel::task::ParallelTask;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
pub(crate) struct Scheduler {
    injector: Arc<Injector<ParallelTask>>,
    locals: Vec<Mutex<Option<Worker<ParallelTask>>>>,
    stealers: Vec<Stealer<ParallelTask>>,
}

#[allow(dead_code)]
pub(crate) struct WorkerHandle {
    local: Worker<ParallelTask>,
    peers: Vec<Stealer<ParallelTask>>,
    injector: Arc<Injector<ParallelTask>>,
}

#[allow(dead_code)]
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
        }
    }

    pub(crate) fn push_inject(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
    }

    pub(crate) fn push_spill(&self, task: ParallelTask, control: &GlobalControl) {
        if !control.accepts_new_work() {
            return;
        }

        control.task_spawned();
        self.injector.push(task);
    }
}

#[allow(dead_code)]
impl WorkerHandle {
    pub(crate) fn pop(&mut self) -> Option<ParallelTask> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel::task::SubtreeTask;
    use std::path::PathBuf;

    #[test]
    fn steal_path_drains_injected_work_after_local_queue_empties() {
        let scheduler = Scheduler::new(2);
        let control = GlobalControl::new();
        scheduler.push_inject(
            ParallelTask::PreOrder(SubtreeTask::new(PathBuf::from("root-a"), 0)),
            &control,
        );
        scheduler.push_inject(
            ParallelTask::PreOrder(SubtreeTask::new(PathBuf::from("root-b"), 0)),
            &control,
        );

        let mut worker0 = scheduler.worker_handle(0);
        let mut worker1 = scheduler.worker_handle(1);

        assert!(matches!(worker0.pop(), Some(ParallelTask::PreOrder(_))));
        assert!(matches!(worker1.pop(), Some(ParallelTask::PreOrder(_))));
    }

    #[test]
    fn quit_state_prevents_new_spill_tasks() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        control.request_quit();

        scheduler.push_spill(
            ParallelTask::PreOrder(SubtreeTask::new(PathBuf::from("child"), 1)),
            &control,
        );
        let mut worker = scheduler.worker_handle(0);

        assert!(worker.pop().is_none());
    }

    #[test]
    fn outstanding_task_count_hits_zero_only_after_finish() {
        let scheduler = Scheduler::new(1);
        let control = GlobalControl::new();
        scheduler.push_inject(
            ParallelTask::PreOrder(SubtreeTask::new(PathBuf::from("root"), 0)),
            &control,
        );

        assert_eq!(control.outstanding_tasks(), 1);
        control.task_finished();
        assert_eq!(control.outstanding_tasks(), 0);
    }
}
