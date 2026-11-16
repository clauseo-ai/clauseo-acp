//! The mode layer for a mode a Claude agent moves to itself (a `current_mode_update`): the
//! update is logged as it came, then, when Clauseo doesn't run Claude in that mode,
//! `session/set_mode` puts the session back, and that is logged once it answers.
//!
//! - Forced ask (`agent/forced_ask.rs`): any mode outside `ASK_MODES` (a plan exit the adapter
//!   took into `acceptEdits` or `auto`) goes back to `default` (`forced_ask::ASK_MODE`, never
//!   `auto` or `acceptEdits` here). Meanwhile the guard hook still asks before every tool, so
//!   the race costs no unasked change.
//! - Elsewhere: `auto`, `acceptEdits` and `plan` ([`NORMAL_MODES`]) are kept; any other mode
//!   goes back to the session's own mode: `auto` (`agent::loads::CLAUDE_MODE`), or
//!   `acceptEdits` once auto proved unavailable (`session/claude_mode.rs`). A plan exit approved
//!   with "manually approve edits" (`default`, the option behind the app's "Allow once") means
//!   "go ahead" in the mode the matter runs in; "use auto mode" lands in `auto` and stays; and
//!   `bypassPermissions`, `dontAsk` or an unknown mode, which claude-agent-acp 0.89.1 never
//!   offers, go back too (decision record 0030, "Claude's permission mode").
//!
//! The switch back must be read back ([`read_back`]): a failed switch, or an agent that then
//! reports another mode, stops the conversation with `agent.mode_not_set`.

use clauseo_protocol::{AgentChoiceId, AgentKind, CompactString, ModeChanged, ThreadId};

use super::super::Actor;
use super::agents::call;
use super::inputs::AgentReply;
use super::updates::{agent, commit};

pub(super) mod read_back;

/// The modes a Claude session outside forced ask is kept in when it moves there itself: `auto`,
/// Clauseo's mode, `acceptEdits`, its fallback (and the mode claude-agent-acp itself falls back
/// to when the model cannot run auto), and `plan`, which makes no change. Any other goes back:
/// `default` would ask before every edit after a plan the lawyer approved, bypass skips every
/// ask, and `dontAsk` turns each ask into a refusal the lawyer never sees.
pub(crate) const NORMAL_MODES: [&str; 3] = [
    crate::agent::loads::CLAUDE_MODE,
    crate::agent::loads::CLAUDE_FALLBACK_MODE,
    "plan",
];

/// The mode a Claude session that moved to `mode` is switched back to, or `None` to keep it:
/// in forced ask `default`; elsewhere `auto`, or `acceptEdits` when auto is `unavailable`.
pub(crate) fn reset_to(forced_ask: bool, unavailable: bool, mode: &str) -> Option<&'static str> {
    if forced_ask {
        (!crate::agent::forced_ask::mode_allowed(mode))
            .then_some(crate::agent::forced_ask::ASK_MODE)
    } else if NORMAL_MODES.contains(&mode) {
        None
    } else if unavailable {
        Some(crate::agent::loads::CLAUDE_FALLBACK_MODE)
    } else {
        Some(crate::agent::loads::CLAUDE_MODE)
    }
}

/// `mode` came/ from `thread`'s agent: a Claude agent is switched back when [`reset_to`] says.
pub(super) fn reset_if_refused(actor: &mut Actor, thread: ThreadId, mode: &str) {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    // A forced-ask agent is always Claude's (`agents.rs`).
    if !claude && !agent.forced_ask {
        return;
    }
    let unavailable = agent.auto.off();
    let Some(target) = reset_to(agent.forced_ask, unavailable, mode) else {
        return;
    };
    let (Some(session), Ok(back)) = (agent.session.clone(), AgentChoiceId::new(target)) else {
        return;
    };
    let (id, client, forced_ask) = (agent.id, agent.link.client.clone(), agent.forced_ask);
    let (mark, fence) = (agent.modes.mark(), agent.link.fence());
    let matter = actor.threads.get(&thread).map(|entry| entry.row.matter);
    tracing::warn!(
        matter = ?matter,
        mode,
        to = target,
        forced_ask,
        "a Claude session moved to a mode Clauseo doesn't run it in; switching it back"
    );
    call(actor, thread, id, async move {
        let result = client.set_mode(&session, back).await;
        fence.await;
        AgentReply::ModeReset((target, mark), result)
    });
}

/// The switch back to `mode`, sent at `mark`, answered: once the agent reported `mode` it is
/// logged (a failed commit is a warning); a failed switch, or another mode reported, stops the
/// conversation with `agent.mode_not_set` ([`read_back::stop`]).
pub(super) fn on_mode_reset(
    actor: &mut Actor,
    thread: ThreadId,
    same: bool,
    (mode, mark): (&'static str, read_back::Mark),
    result: Result<(), clauseo_acp::Error>,
) {
    if !same {
        return;
    }
    let checked = match result {
        Ok(()) => read_back::check(actor, thread, mode, mark),
        Err(error) => {
            let cause = crate::log_file::Cause(&error).to_string();
            tracing::warn!(%thread, mode, error = cause, "a Claude session refused its switch back");
            Err(read_back::not_set(thread, mode, None))
        }
    };
    if let Err(error) = checked {
        read_back::stop(actor, thread, &error);
        return;
    }
    let mode_id = CompactString::from(mode);
    let logged = commit(actor, thread, [ModeChanged { mode_id }.into()], Vec::new());
    if let Err(error) = logged {
        tracing::warn!(
            %thread,
            mode,
            error = %crate::log_file::Cause(&error),
            "a Claude session was not switched back"
        );
    }
}

#[cfg(test)]
mod tests;
