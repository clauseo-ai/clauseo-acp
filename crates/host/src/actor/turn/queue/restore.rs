//! Start-up: a thread's queue rebuilt from its whole log, held. Its queued turns are the
//! prompts whose turn neither started nor ended, in the order they joined the queue (a prompt's
//! `UserMessage`, a requeued steer's `SteerResolved`), with their edits applied.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    ContentBlock, DomainEvent, Envelope, MessageId, SteerOutcome, ThreadId, TurnId, TurnQueued,
};

use super::super::super::Actor;
use super::super::out::Out;
use super::super::reconcile::{needs_queued, previous_turn};
use super::super::{lost, state};
use super::{Queued, sync_held};
use crate::error::Error;

/// A prompt as the log leaves it.
struct Prompt {
    /// Where it joined the queue in the log.
    at: usize,
    turn: TurnId,
    content: Vec<ContentBlock>,
}

/// Rebuilds `thread`'s queue from `log`, held, and pushes into `out` a `TurnQueued` behind
/// its own thread (behind the thread's previous turn) for each queued turn whose last
/// `TurnQueued` names another thread or is missing, so the view shows it queued.
pub(in super::super) fn restore(
    actor: &mut Actor,
    thread: ThreadId,
    log: &[Envelope],
    out: &mut Out,
) -> Result<(), Error> {
    let queued = queued_turns(log);
    for (turn, _, _) in &queued {
        if needs_queued(log, thread, *turn) {
            let waits = TurnQueued::new(thread, *turn, thread, previous_turn(log, *turn))
                .map_err(|error| Error::Internal(error.to_string()))?;
            out.push(actor, waits)?;
        }
    }
    let queue = &mut state(actor, thread).ok_or_else(|| lost("a thread"))?.queue;
    queue.turns = queued
        .into_iter()
        .map(|(turn, message, content)| Queued {
            turn,
            message,
            content,
            forced: false,
        })
        .collect();
    queue.held = !queue.turns.is_empty();
    sync_held(actor, thread);
    Ok(())
}

/// The queued turns of `log` in queue order: turn, message, content as step 5 sends it.
fn queued_turns(log: &[Envelope]) -> Vec<(TurnId, MessageId, Vec<ContentBlock>)> {
    let mut prompts: BTreeMap<MessageId, Prompt> = BTreeMap::new();
    let mut done: BTreeSet<TurnId> = BTreeSet::new();
    for (at, envelope) in log.iter().enumerate() {
        let event = &envelope.event;
        if let DomainEvent::UserMessage(message) = event {
            let prompt = Prompt {
                at,
                turn: message.turn,
                content: message.content.clone(),
            };
            prompts.insert(message.message, prompt);
        } else if let DomainEvent::QueuedPromptEdited(edited) = event
            && let Some(prompt) = prompts.get_mut(&edited.message)
        {
            prompt.content.clone_from(&edited.content);
        } else if let DomainEvent::SteerResolved(resolved) = event
            && let Some(prompt) = prompts.get_mut(&resolved.message)
            && let SteerOutcome::Requeued { turn } | SteerOutcome::SentNow { turn } =
                &resolved.outcome
        {
            prompt.turn = *turn;
            prompt.at = at;
        } else if let DomainEvent::TurnStarted(started) = event {
            done.insert(started.turn);
        } else if let DomainEvent::TurnEnded(ended) = event {
            done.insert(ended.turn);
        }
    }
    let mut queued: Vec<(usize, TurnId, MessageId, Vec<ContentBlock>)> = prompts
        .into_iter()
        .filter(|(_, prompt)| !done.contains(&prompt.turn))
        .map(|(message, prompt)| (prompt.at, prompt.turn, message, prompt.content))
        .collect();
    queued.sort_by_key(|(at, ..)| *at);
    queued
        .into_iter()
        .map(|(_, turn, message, content)| (turn, message, content))
        .collect()
}
