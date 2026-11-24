//! The model probe's part in the host: once per agent kind after each good limits read that
//! follows the host's start or a sign-in, the reader probes the agent's models
//! (`ReadLimits::probe_models`, `clauseo_limits`' `readers/models.rs`) off the actor, and the
//! answer is kept as the agent's (`AgentOptions::keep_probed`), so a new conversation's pickers
//! name the real models before its first message.
//!
//! A good read is one that found the agent signed in (its usage, or Claude's usage method
//! missing), so a probe never runs for an agent that is not installed and signed in, nor under
//! `sandbox.required`, which refuses every read. The probe sends no prompt, makes no
//! conversation and starts no MCP server. Nothing waits on it: not the host's start, not a
//! launch.
//!
//! One that fails or times out keeps whatever was kept, and the next good read probes again, up
//! to [`TRIES`] probes since the start or the last sign-in (a first start of Claude Code can
//! pass the probe's limit). While no probe has answered and nothing is kept for the agent, its
//! `AgentSummary::models_unread` says so ([`unread`]), so the composer can say the list could
//! not be read instead of waiting on it; a probe or a session that fills its models clears it.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_limits::ReadOutcome;
use clauseo_protocol::{AgentKind, AgentModels};

use super::super::Actor;
use super::super::input::Input;
use super::super::limits::LimitsInput;

/// The most probes of an agent kind since the host's start or its last sign-in.
pub(crate) const TRIES: u8 = 3;

/// Which agent kinds have a probe running or answered since the host's start or their last
/// sign-in, and how each kind's probes failed.
#[derive(Debug, Default)]
pub(crate) struct Probes {
    started: BTreeSet<AgentKind>,
    /// Failed probes per kind since the start or its last sign-in.
    failed: BTreeMap<AgentKind, u8>,
}

impl Probes {
    /// Whether `agent`'s last probe failed (none has answered since).
    fn failed(&self, agent: AgentKind) -> bool {
        self.failed.get(&agent).is_some_and(|count| *count > 0)
    }
}

/// `AgentSummary::models_unread` for the agent `id`: its last probe failed and nothing fills
/// its models (`models`, the summary's).
pub(crate) fn unread(
    probes: &Probes,
    id: &str,
    models: Option<&clauseo_protocol::AgentModels>,
) -> bool {
    let kind = clauseo_protocol::AgentId::new(id)
        .ok()
        .and_then(|id| AgentKind::of(&id));
    models.is_none() && kind.is_some_and(|kind| probes.failed(kind))
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

/// `agent`'s login changed: its next good read probes it again, with all its tries.
pub(crate) fn login_changed(actor: &mut Actor, agent: AgentKind) {
    let probes = &mut actor.agent_options.probes;
    probes.started.remove(&agent);
    probes.failed.remove(&agent);
}

/// `agent`'s probe answered: its models are kept, and every app subscriber told when that
/// changed them. A failed probe (`None`) keeps what was kept, and leaves the next good read to
/// probe again while tries are left; subscribers are told when that makes the models unread.
pub(crate) fn on_probed(actor: &mut Actor, agent: AgentKind, models: Option<AgentModels>) {
    let probes = &mut actor.agent_options.probes;
    let Some(models) = models else {
        let was = probes.failed(agent);
        let count = probes.failed.entry(agent).or_insert(0);
        *count = count.saturating_add(1);
        let retry = *count < TRIES;
        if retry {
            probes.started.remove(&agent);
        }
        tracing::info!(
            ?agent,
            retry,
            "model probe found nothing; the kept models stay"
        );
        let unread = actor.agent_options.models_of(agent.id()).is_none();
        super::told(actor, Ok(!was && unread));
        return;
    };
    let was = probes.failed.remove(&agent).is_some();
    tracing::info!(?agent, models = models.models.len(), "model probe answered");
    let kept = actor.agent_options.keep_probed(agent.id(), models);
    let kept = kept.map(|changed| changed || was);
    super::told(actor, kept);
}
