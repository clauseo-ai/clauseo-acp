//! A permission request Clauseo's data fence refused (`agent/data_fence.rs`): the agent gets the
//! reject-once option, else `cancelled`, and the lawyer is never asked. ACP's answer carries no
//! reason, and claude-agent-acp tells the model a reject is the user's ("User refused permission
//! to run tool", 0.89.1 `permissions/effects.js`), so the host says what happened itself, once a
//! turn: a host `Notice` (`NoticeKind::OutsideMatterBlocked`) for the lawyer, and, to an agent
//! whose steer joins the running turn (the lawyer's steer decides the same way,
//! `queue/steer.rs`), [`AGENT_NOTE`] as a steer, never logged: it is no lawyer's message, and no
//! prompt the queue, the idle stop or the limits count. An agent without steering hears only its
//! adapter's words; Codex then cancels its turn, which ends `OutsideMatter`.
//!
//! A race is accepted: an agent that ends its turn before the note arrives answers the steer
//! `PromptRequired`, and the note is lost (never sent as a new prompt); the notice alone then
//! corrects the adapter's "you declined" for the lawyer. An agent that answers
//! `StartedNewTurn` runs a turn the host does not track, which is logged as a warning.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentChoiceId, ContentBlock, Notice, NoticeKind, NoticeLevel, StopReason, ThreadId,
};

use super::super::Actor;
use super::asks::PermissionAsk;
use super::shell_network::reject_option;
use super::updates::{agent, commit};
use super::{current, spawn};
use crate::log_file::Cause;

/// What the agent is told after the refusal, as the next message of its running turn.
pub(super) const AGENT_NOTE: &str =
    "Clauseo blocked that request: it reaches outside this matter. The lawyer did not decline it.";

/// The notice's fallback words, for a view that does not know its kind (TN-12's English: the
/// view names the agent, TN-11).
pub(super) const NOTICE_TEXT: &str =
    "Clauseo stopped a step: the AI tried to reach outside this matter.";

/// Rejects a request that touches Clauseo's data folder, unasked: the agent's count for its
/// `agent ended` line goes up and no path is logged. The first refusal of the turn also logs
/// the notice and, after the answer and in the same task (so the agent reads it after the
/// refusal), sends the note ([`tell_once`]). An agent that cancels its turn on it (Codex) ends
/// the turn `OutsideMatter`, not the lawyer's `Cancelled`.
pub(super) fn refuse_data_folder(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) {
    let reject =
        reject_option(&ask.options).and_then(|option| AgentChoiceId::new(&option.option_id).ok());
    if let Some(current) = current(actor, thread) {
        current.refused = Some(StopReason::OutsideMatter);
    }
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    agent.denied_data_folder = agent.denied_data_folder.saturating_add(1);
    let client = agent.link.client.clone();
    let key = ask.key;
    let note = tell_once(actor, thread).then(|| ask.session.clone());
    spawn(actor, async move {
        let answered = match reject {
            Some(option) => client.answer_permission(key, option).await,
            None => client.cancel_permission(key).await,
        };
        if let Err(error) = answered {
            tracing::warn!(error = %Cause(&error), "a data-folder refusal did not reach the agent");
            return;
        }
        let Some(session) = note else {
            return;
        };
        let text = AGENT_NOTE.to_owned();
        match client
            .steer(&session, vec![ContentBlock::Text { text }])
            .await
        {
            Ok(SteerResult::StartedNewTurn) => {
                tracing::warn!("the data-folder note started a turn the host does not track");
            }
            Ok(result) => tracing::debug!(?result, "the data-folder note reached the agent"),
            Err(error) => {
                let error = Cause(&error);
                tracing::warn!(%error, "the data-folder note did not reach the agent");
            }
        }
    });
}

/// The first time in the live turn, logs the notice and says whether the agent gets the note:
/// when a real steer would join the running turn (`queue::steer::Target::Join`).
/// Later refusals of the same turn (an agent retrying the request) say nothing more.
fn tell_once(actor: &mut Actor, thread: ThreadId) -> bool {
    let Some(current) = current(actor, thread) else {
        return false;
    };
    if std::mem::replace(&mut current.told.outside, true) {
        return false;
    }
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: NOTICE_TEXT.to_owned(),
        kind: Some(NoticeKind::OutsideMatterBlocked),
    };
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "a data-folder notice was not logged");
    }
    // Only into a prompting turn that was not stopped, as the lawyer's own steer.
    matches!(
        super::queue::steer::target(actor, thread),
        super::queue::steer::Target::Join(_)
    )
}
