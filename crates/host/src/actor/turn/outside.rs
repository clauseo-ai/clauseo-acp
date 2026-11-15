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
//! A request that reaches another matter (`agent::matter_fence`, `PathsChecked::OtherMatter`),
//! and a tool call that failed as refused on a path in one (`refused_call.rs`), are told once a
//! turn for each matter: the notice is `NoticeKind::OtherMatterRefused`, naming the matter, so
//! the app can offer to let the conversation read it (`OutsideMatterBlocked` again for a matter
//! the conversation may read already), and the agent's note ([`OTHER_MATTER_NOTE`]) tells it
//! to tell the lawyer that it is another matter's folder.
//!
//! A race is accepted: an agent that ends its turn before the note arrives answers the steer
//! `PromptRequired`, and the note is lost (never sent as a new prompt); the notice alone then
//! corrects the adapter's "you declined" for the lawyer. An agent that answers
//! `StartedNewTurn` runs a turn the host does not track, which is logged as a warning.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentChoiceId, ContentBlock, MatterId, Notice, NoticeKind, NoticeLevel, StopReason, ThreadId,
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

/// What the agent is told after a refusal for another matter's folder (provisional words,
/// `b4l-mfence.md`).
pub(super) const OTHER_MATTER_NOTE: &str = "Clauseo blocked that request: it reaches another \
    matter's folder. Tell the lawyer that folder belongs to another matter; they can let this \
    conversation read that matter's files. The lawyer did not decline it.";

/// The other-matter notice's fallback words, for a view that does not know its kind.
pub(super) const OTHER_MATTER_TEXT: &str =
    "Clauseo stopped a step: the AI tried to open another matter's files.";

/// The notice's fallback words, for a view that does not know its kind (TN-12's English: the
/// view names the agent, TN-11).
pub(super) const NOTICE_TEXT: &str =
    "Clauseo stopped a step: the AI tried to reach outside this matter.";

/// Rejects a request that touches Clauseo's data folder, or another matter when `matter` names
/// it, unasked: the agent's count for its `agent ended` line goes up and no path is logged. The first refusal of the turn also logs
/// the notice and, after the answer and in the same task (so the agent reads it after the
/// refusal), sends the note ([`tell_once`]). An agent that cancels its turn on it (Codex) ends
/// the turn `OutsideMatter`, not the lawyer's `Cancelled`.
pub(super) fn refuse_data_folder(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
    matter: Option<MatterId>,
) {
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
    let told = match matter {
        Some(matter) => tell_other_matter(actor, thread, matter),
        None => tell_once(actor, thread),
    };
    let note = told.then(|| ask.session.clone());
    let words = if matter.is_some() {
        OTHER_MATTER_NOTE
    } else {
        AGENT_NOTE
    };
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
        let text = words.to_owned();
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

/// The first time in the live turn for `matter`, logs the other-matter notice (the matter's
/// name as the host knows it now) and says whether the agent gets the note, as [`tell_once`].
/// A matter the conversation may read already gets the plain outside-the-matter notice: there
/// is nothing to offer.
pub(super) fn tell_other_matter(actor: &mut Actor, thread: ThreadId, matter: MatterId) -> bool {
    let invited = super::fence::read_matters(actor, thread).contains(&matter);
    let name = actor
        .matters
        .get(&matter)
        .or_else(|| actor.forgotten.get(&matter))
        .map(|entry| entry.row.name.clone());
    let Some(current) = current(actor, thread) else {
        return false;
    };
    if current.told.other_matters.contains(&matter) {
        return false;
    }
    current.told.other_matters.push(matter);
    let notice = match name {
        Some(name) if !invited => Notice {
            level: NoticeLevel::Warning,
            text: OTHER_MATTER_TEXT.to_owned(),
            kind: Some(NoticeKind::OtherMatterRefused { matter, name }),
        },
        _ => Notice {
            level: NoticeLevel::Warning,
            text: NOTICE_TEXT.to_owned(),
            kind: Some(NoticeKind::OutsideMatterBlocked),
        },
    };
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "an other-matter notice was not logged");
    }
    matches!(
        super::queue::steer::target(actor, thread),
        super::queue::steer::Target::Join(_)
    )
}
