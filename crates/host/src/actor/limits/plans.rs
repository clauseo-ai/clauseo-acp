//! Plans set up after start-up, and the info lines that trace a plan's reads.
//!
//! [`super::start`] builds the model once, from the settings at start-up. A plan whose entry is
//! set later in the same run (setup's `set_agent_launch`) joins here: the model is built if
//! there was none, the agent joins loading with its read, and the view goes out. A model built
//! here while windows already subscribe turns `Watching` on, so they get the view and the
//! deadlines run, as the first subscriber does in [`super::subscribe`].

use clauseo_limits::{Input as ModelInput, Limits, ReadOutcome};
use clauseo_protocol::AgentKind;

use super::{Actor, carry_out, feed, rearm};

/// `agent`'s plan was set up: the model reads it from now on. An agent the model has already
/// changes nothing.
pub(crate) fn plan_added(actor: &mut Actor, agent: AgentKind) {
    if actor.limits.agents.contains(&agent) {
        return;
    }
    tracing::info!(?agent, "limits: plan added, reading");
    let now = actor.now().wall;
    actor.limits.agents.push(agent);
    let actions = if let Some(model) = actor.limits.model.as_mut() {
        model.add_agent(agent, now)
    } else {
        let (model, actions) = Limits::new(Vec::new(), &[agent], now);
        actor.limits.model = Some(model);
        actions
    };
    carry_out(actor, actions, now);
    if !actor.limits.watching && !actor.limits.subs.is_empty() {
        // The model is new and windows already watch: `Watching` answers with the view.
        actor.limits.watching = true;
        feed(actor, ModelInput::Watching { any: true });
    }
    rearm(actor, now);
}

/// How a read ended, in a word for the log.
pub(super) const fn outcome_word(outcome: &ReadOutcome) -> &'static str {
    match outcome {
        ReadOutcome::Usage { .. } => "usage",
        ReadOutcome::MethodMissing => "method_missing",
        ReadOutcome::SignedOut => "signed_out",
        ReadOutcome::Failed(_) => "failed",
    }
}

/// Logs a read of `agent` that ended as `outcome` ([`outcome_word`]), and the state the
/// model shows for it after taking it.
pub(super) fn log_read_done(actor: &Actor, agent: AgentKind, outcome: &'static str) {
    let now = actor.now().wall;
    let freshness = actor
        .limits
        .model
        .as_ref()
        .and_then(|model| model.view(now).0.remove(&agent))
        .map(|view| view.freshness);
    tracing::info!(?agent, outcome, ?freshness, "limits: read done");
}
