//! An agent-started turn refused before it started (its before-turn checkpoint refused: the
//! matter's folder moved, `matter.folder_missing`; a failed checkpoint, `matter.checkpoint_failed`,
//! as when a file is held with no read sharing; or its start not logged). The output it was opened
//! for is never kept to open another turn: `queue::on_turn_closed` calls `open_if_idle` as every
//! turn closes, and output still held would open the next agent-started turn at once, be refused
//! the same way, and loop at the commit rate (three thread events a pass, about 1 MiB a minute)
//! while the refusal lasted. So the held updates are dropped with a host.log line, and the held
//! cards (permission requests, elicitations) are answered at once: the permission's reject-once
//! option (else `cancelled`), an elicitation `decline`. ACP's answer carries no reason, so the
//! reason goes to host.log. A refused agent-started turn's agent is stopped next
//! (`steps::on_before_turn`), once those answers are on its connection ([`stop_agent`]): the
//! stop closes the agent's stdin at once, and an answer still on its way would never be read. A
//! refused prompt of the lawyer's stops nothing.
//!
//! The belt ([`CAP`]): however a thread's agent-started turns come to fail, no more than `CAP`
//! of them in a row fail with the same code and no turn starting between: the `CAP`th stops the
//! agent, once, and is logged. A turn that starts, or a turn the lawyer sent, ends the run.

use clauseo_acp::{AcpEvent, PendingKey};
use clauseo_protocol::{AgentChoiceId, ElicitationResponse, ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::asks::reject_option;
use super::super::inputs::AgentReply;
use super::super::{agents, state, updates};
use super::is_own;

/// The most agent-started turns of a thread that fail in a row with the same code, no turn
/// starting between, before its agent is stopped.
pub(in crate::actor::turn) const CAP: u32 = 3;

/// `thread`'s current turn failed before it started with `code`: the output held for an
/// agent-started turn is dropped and its cards answered (the module doc), whether this turn was
/// agent-started or a prompt the output came during (whose close would open one for it). The
/// run of refused agent-started turns is counted ([`CAP`]); a prompted turn, the lawyer's,
/// ends it.
pub(in crate::actor::turn) fn unstarted_failed(
    actor: &mut Actor,
    thread: ThreadId,
    code: ErrorCode,
) {
    let own = is_own(actor, thread);
    let Some(state) = state(actor, thread) else {
        return;
    };
    let held = std::mem::take(&mut state.own_held.updates);
    let run = match state.own_held.failed {
        Some((last, count)) if own && last == code => count.saturating_add(1),
        _ if own => 1,
        _ => 0,
    };
    state.own_held.failed = (run > 0).then_some((code, run));
    if !held.is_empty() {
        let cards = held.iter().filter(|event| is_card(event)).count();
        tracing::warn!(
            %thread,
            %code,
            dropped = held.len(),
            cards,
            "a turn was refused before it started; the agent output held for a turn is dropped \
             and its cards answered: Clauseo couldn't show them to the lawyer"
        );
        answer(actor, thread, &held);
    }
    if run >= CAP {
        tracing::warn!(%thread, %code, run, "agent-started turns keep failing; the agent is stopped");
        stop_agent(actor, thread);
    }
}

/// Stops `thread`'s agent once the held cards answered just now have been handed to its
/// connection: the stop closes the agent's stdin at once, which would beat an answer still
/// on its way, so the agent would never read it.
pub(in crate::actor::turn) fn stop_agent(actor: &mut Actor, thread: ThreadId) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    if state.own_held.answering > 0 {
        state.own_held.stop_after = true;
        return;
    }
    agents::stop(actor, thread);
}

/// The answers to held cards that went to `thread`'s agent (`same`: the agent the thread still
/// has) were handed to its connection: a stop that waited for them goes now.
pub(in crate::actor::turn) fn answered(actor: &mut Actor, thread: ThreadId, same: bool) {
    let Some(state) = state(actor, thread).filter(|_| same) else {
        return;
    };
    let held = &mut state.own_held;
    held.answering = held.answering.saturating_sub(1);
    if held.answering == 0 && std::mem::take(&mut held.stop_after) {
        agents::stop(actor, thread);
    }
}

/// `thread`'s agent ended: what it sent between turns is dropped (no card of it is answered on
/// another connection, where its key may name another request), and the run of failed
/// agent-started turns ends with it.
pub(in crate::actor::turn) fn agent_ended(actor: &mut Actor, thread: ThreadId) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    state.own_held.failed = None;
    state.own_held.answering = 0;
    state.own_held.stop_after = false;
    let held = std::mem::take(&mut state.own_held.updates);
    if !held.is_empty() {
        tracing::info!(%thread, dropped = held.len(), "the agent ended; what it held went with it");
    }
}

/// How many held cards of `thread` were answered on an agent so far (tests).
#[cfg(test)]
pub(in crate::actor::turn) fn sent(actor: &mut Actor, thread: ThreadId) -> usize {
    state(actor, thread).map_or(0, |state| state.own_held.sent)
}

/// A turn of `thread` started: the run of failed agent-started turns ends.
pub(in crate::actor::turn) fn started(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.own_held.failed = None;
    }
}

fn is_card(event: &AcpEvent) -> bool {
    matches!(
        event,
        AcpEvent::PermissionRequested { .. } | AcpEvent::ElicitationRequested { .. }
    )
}

/// What a held card is answered with.
enum Answer {
    Option(PendingKey, AgentChoiceId),
    Cancel(PendingKey),
    Decline(PendingKey),
}

/// What `card` is answered with, as the module doc says; `None` for an update.
fn answer_of(card: &AcpEvent) -> Option<Answer> {
    if let AcpEvent::PermissionRequested { key, options, .. } = card {
        let reject =
            reject_option(options).and_then(|option| AgentChoiceId::new(&option.option_id).ok());
        return Some(reject.map_or(Answer::Cancel(*key), |option| Answer::Option(*key, option)));
    }
    if let AcpEvent::ElicitationRequested { key, .. } = card {
        return Some(Answer::Decline(*key));
    }
    None
}

/// Answers the cards among `held` off the actor ([`answer_of`]); once they are handed to the
/// connection, [`answered`] runs.
fn answer(actor: &mut Actor, thread: ThreadId, held: &[AcpEvent]) {
    let answers: Vec<Answer> = held.iter().filter_map(answer_of).collect();
    if answers.is_empty() {
        return;
    }
    let Some(agent) = updates::agent(actor, thread) else {
        return;
    };
    let client = agent.link.client.clone();
    let id = agent.id;
    if let Some(state) = state(actor, thread) {
        state.own_held.answering = state.own_held.answering.saturating_add(1);
        #[cfg(test)]
        {
            state.own_held.sent += answers.len();
        }
    }
    agents::call(actor, thread, id, async move {
        for answer in answers {
            let answered = match answer {
                Answer::Option(key, option) => client.answer_permission(key, option).await,
                Answer::Cancel(key) => client.cancel_permission(key).await,
                Answer::Decline(key) => {
                    client
                        .answer_elicitation(key, ElicitationResponse::Decline)
                        .await
                }
            };
            if let Err(error) = answered {
                tracing::debug!(%error, "a held card of a refused agent-started turn was not answered");
            }
        }
        AgentReply::HeldAnswered
    });
}
