//! The model probe's part in the host: once per agent kind after each good limits read that
//! follows the host's start or a sign-in, the reader probes the agent's models
//! (`ReadLimits::probe_models`, `clauseo_limits`' `readers/models.rs`) off the actor, and the
//! answer is kept as the agent's (`AgentOptions::keep_probed`), so a new conversation's pickers
//! name the real models before its first message.
//!
//! A good read is one that found the agent signed in (its usage, or Claude's usage method
//! missing), so a probe never runs for an agent that is not installed and signed in, nor under
//! `sandbox.required`, which refuses every read. The probe sends no prompt, makes no
//! conversation and starts no MCP server. One that fails or times out keeps whatever was kept,
//! and says nothing to the lawyer. Nothing waits on it: not the host's start, not a launch.

use std::collections::BTreeSet;

use clauseo_limits::ReadOutcome;
use clauseo_protocol::{AgentKind, AgentModels};

use super::super::Actor;
use super::super::input::Input;
use super::super::limits::LimitsInput;

/// Which agent kinds have had their probe (or have one running) since the host's start or
/// their last sign-in.
#[derive(Debug, Default)]
pub(crate) struct Probes {
    started: BTreeSet<AgentKind>,
}

/// A limits read of `agent` ended with `outcome`: its probe starts when the read found it
/// signed in and none has started since the start or its last sign-in.
pub(crate) fn after_read(actor: &mut Actor, agent: AgentKind, outcome: &ReadOutcome) {
    let signed_in = matches!(
        outcome,
        ReadOutcome::Usage { .. } | ReadOutcome::MethodMissing
    );
    if !signed_in || !actor.agent_options.probes.started.insert(agent) {
        return;
    }
    tracing::info!(?agent, "model probe started");
    let probe = super::super::limits::reader(actor).probe_models(agent);
    actor.spawn_post(async move {
        let models = probe.await;
        Input::Limits(LimitsInput::Probed { agent, models })
    });
}

/// `agent`'s login changed: its next good read probes it again.
pub(crate) fn login_changed(actor: &mut Actor, agent: AgentKind) {
    actor.agent_options.probes.started.remove(&agent);
}

/// `agent`'s probe answered: its models are kept, and every app subscriber told when that
/// changed them; a failed probe (`None`) keeps what was kept.
pub(crate) fn on_probed(actor: &mut Actor, agent: AgentKind, models: Option<AgentModels>) {
    let Some(models) = models else {
        tracing::info!(?agent, "model probe found nothing; the kept models stay");
        return;
    };
    tracing::info!(?agent, models = models.models.len(), "model probe answered");
    let kept = actor.agent_options.keep_probed(agent.id(), models);
    super::told(actor, kept);
}
