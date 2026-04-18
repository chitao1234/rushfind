use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[allow(dead_code)]
pub(crate) struct GlobalControl {
    accept_new_work: AtomicBool,
    quit_seen: AtomicBool,
    fatal_error_seen: AtomicBool,
    outstanding_tasks: AtomicUsize,
}

#[allow(dead_code)]
impl GlobalControl {
    pub(crate) fn new() -> Self {
        Self {
            accept_new_work: AtomicBool::new(true),
            quit_seen: AtomicBool::new(false),
            fatal_error_seen: AtomicBool::new(false),
            outstanding_tasks: AtomicUsize::new(0),
        }
    }

    pub(crate) fn accepts_new_work(&self) -> bool {
        self.accept_new_work.load(Ordering::SeqCst)
    }

    pub(crate) fn quit_seen(&self) -> bool {
        self.quit_seen.load(Ordering::SeqCst)
    }

    pub(crate) fn fatal_error_seen(&self) -> bool {
        self.fatal_error_seen.load(Ordering::SeqCst)
    }

    pub(crate) fn request_quit(&self) {
        self.quit_seen.store(true, Ordering::SeqCst);
        self.accept_new_work.store(false, Ordering::SeqCst);
    }

    pub(crate) fn request_fatal_stop(&self) {
        self.fatal_error_seen.store(true, Ordering::SeqCst);
        self.accept_new_work.store(false, Ordering::SeqCst);
    }

    pub(crate) fn task_spawned(&self) {
        self.outstanding_tasks.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn task_finished(&self) {
        self.outstanding_tasks.fetch_sub(1, Ordering::SeqCst);
    }

    pub(crate) fn outstanding_tasks(&self) -> usize {
        self.outstanding_tasks.load(Ordering::SeqCst)
    }
}
