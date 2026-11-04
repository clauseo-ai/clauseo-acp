//! Forced ask's mode layer for a mode the agent moves to itself (`agent/forced_ask.rs`): a
//! `current_mode_update` naming a mode outside `ASK_MODES` (a plan exit the adapter took into
//! `acceptEdits`) is logged as it came, then `session/set_mode` puts the session back in
//! `default`, and that is logged once it answers. Meanwhile the guard hook still asks before
//! every tool, so the race costs no unasked change.

use clauseo_protocol::{AgentChoiceId, CompactString, ModeChanged, ThreadId};

use super::super::Actor;
use super::agents::call;
use super::inputs::AgentReply;
use super::updates::{agent, commit};

/// `mode` came from a forced-ask agent: switch it back when it is not allowed.
pub(super) fn reset_if_forced(actor: &mut Actor, thread: ThreadId, mode: &str) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    if !agent.forced_ask || crate::agent::forced_ask::mode_allowed(mode) {
        return;
    }
    let (Some(session), Ok(default)) = (
        agent.session.clone(),
        AgentChoiceId::new(crate::agent::loads::CLAUDE_MODE),
    ) else {
        return;
    };
    let (id, client) = (agent.id, agent.link.client.clone());
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    tracing::info!(
        matter = ?matter,
        mode,
        "a forced-ask Claude session moved to a mode that skips asks; switching it back"
    );
    call(actor, thread, id, async move {
        AgentReply::ModeReset(client.set_mode(&session, default).await)
    });
}

/// The switch back answered: `default` is logged; a failure is a warning (the hook still asks).
pub(super) fn on_mode_reset(
    actor: &mut Actor,
    thread: ThreadId,
    same: bool,
    result: Result<(), clauseo_acp::Error>,
) {
    if !same {
        return;
    }
    let logged = result.map_err(crate::error::Error::Acp).and_then(|()| {
        let mode_id = CompactString::from(crate::agent::loads::CLAUDE_MODE);
        commit(actor, thread, [ModeChanged { mode_id }.into()], Vec::new())
    });
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "a forced-ask session was not switched back to default");
    }
}
