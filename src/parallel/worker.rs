use crate::diagnostics::Diagnostic;
use crate::entry::EntryContext;
use crate::eval::{
    ActionOutcome, ActionSink, EvalContext, RuntimeStatus, evaluate_outcome_with_context,
};
use crate::follow::FollowMode;
use crate::output::render_runtime_action_bytes;
use crate::parallel::broker::BrokerMessage;
use crate::planner::{ExecutionPlan, RuntimeAction};
use crossbeam_channel::Sender;

pub(crate) struct OutputOnlySink {
    broker: Sender<BrokerMessage>,
}

impl OutputOnlySink {
    pub(crate) fn new(broker: Sender<BrokerMessage>) -> Self {
        Self { broker }
    }
}

impl ActionSink for OutputOnlySink {
    fn dispatch(
        &mut self,
        action: &RuntimeAction,
        entry: &EntryContext,
        follow_mode: FollowMode,
    ) -> Result<ActionOutcome, Diagnostic> {
        let bytes = render_runtime_action_bytes(action, entry, follow_mode)?;
        self.broker
            .send(BrokerMessage::Stdout(bytes))
            .map_err(|_| Diagnostic::new("internal error: v2 broker is unavailable", 1))?;
        Ok(ActionOutcome::matched_true())
    }
}

pub(crate) fn process_entry_output_only(
    plan: &ExecutionPlan,
    entry: &EntryContext,
    follow_mode: FollowMode,
    context: &EvalContext,
    broker: &Sender<BrokerMessage>,
) -> Result<RuntimeStatus, Diagnostic> {
    let mut sink = OutputOnlySink::new(broker.clone());
    let outcome =
        evaluate_outcome_with_context(&plan.expr, entry, follow_mode, context, &mut sink)?;
    Ok(outcome.status)
}
