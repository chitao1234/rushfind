use crate::diagnostics::Diagnostic;
use crate::eval::RuntimeStatus;
use crate::exec::{
    BatchLimit, BatchedExecAction, ExecBatchKey, PendingBatch, ReadyBatch, fixed_batch_cost,
    run_parallel_ready_batch,
};
use crate::parallel::broker::BrokerMessage;
use crossbeam_channel::Sender;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) struct WorkerBatchState {
    pending: BTreeMap<ExecBatchKey, PendingBatch>,
    spill_threshold: usize,
    batch_limit: BatchLimit,
}

impl WorkerBatchState {
    pub(crate) fn new(spill_threshold: usize) -> Self {
        Self {
            pending: BTreeMap::new(),
            spill_threshold,
            batch_limit: BatchLimit::detect(),
        }
    }

    pub(crate) fn enqueue(
        &mut self,
        spec: &BatchedExecAction,
        path: &Path,
        broker: &Sender<BrokerMessage>,
    ) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = RuntimeStatus::default();
        let key = ExecBatchKey {
            id: spec.id,
            cwd: spec.batch_cwd(path),
        };
        let batch = self.pending.entry(key).or_insert_with(|| {
            PendingBatch::new(spec.clone(), self.batch_limit, fixed_batch_cost(spec))
        });

        if !batch.paths.is_empty() && batch.would_overflow(path) {
            let ready = batch.take_ready();
            if !run_parallel_ready_batch(&ready, broker, self.spill_threshold)? {
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        if let Some(ready) = batch.push(path)?
            && !run_parallel_ready_batch(&ready, broker, self.spill_threshold)?
        {
            status = status.merge(RuntimeStatus::action_failure());
        }

        Ok(status)
    }

    pub(crate) fn flush_all(
        &mut self,
        broker: &Sender<BrokerMessage>,
    ) -> Result<RuntimeStatus, Diagnostic> {
        let mut status = RuntimeStatus::default();
        for (_, batch) in std::mem::take(&mut self.pending) {
            if batch.paths.is_empty() {
                continue;
            }

            let ready = ReadyBatch {
                spec: batch.spec,
                paths: batch.paths,
            };
            if !run_parallel_ready_batch(&ready, broker, self.spill_threshold)? {
                status = status.merge(RuntimeStatus::action_failure());
            }
        }

        Ok(status)
    }
}
