use crate::eval::EvalContext;
use crate::parallel::postorder::BarrierTable;
use crate::planner::ExecutionPlan;
use crate::walker::WalkBackend;

use super::WorkerActionSink;
use crate::parallel::scheduler::WorkerHandle;

#[derive(Clone, Copy)]
pub(super) struct WorkerRunContext<'a> {
    pub(super) plan: &'a ExecutionPlan,
    pub(super) backend: &'a dyn WalkBackend,
    pub(super) barriers: &'a BarrierTable,
    pub(super) eval_context: &'a EvalContext,
}

pub(super) struct PostorderRunContext<'run, 'state> {
    pub(super) run: WorkerRunContext<'run>,
    pub(super) worker: &'state mut WorkerHandle,
    pub(super) sink: &'state mut WorkerActionSink,
    pub(super) had_runtime_errors: &'state mut bool,
}
