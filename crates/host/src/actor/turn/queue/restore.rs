//! Start-up: a thread's queue rebuilt from its whole log, held. Its queued turns are the
//! prompts whose turn neither started nor ended, in the order they joined the queue (a prompt's
//! `UserMessage`, a requeued steer's `SteerResolved`), with their edits applied. A steer saved
//! and never answered (the host stopped first) moves to a new queued turn, so it is not lost; it
//! keeps its message's place in the log, behind any prompt queued before it. The turns put back
//! at the head live keep that place across a restart: a requeued steer (`steer.rs`) and a turn
//! put back for its agent's sign-in (`sign_in.rs`) both have their `TurnQueued` marked
//! `at_head`, so they come back first, the latest put back first, as the live queue has them.
//! The queue comes back held, so nothing runs before the lawyer looks. Only a turn put back for
//! the sign-in marks the queue as waiting for it, so `restart_agents` resumes that one; a steer
//! does not. An older log has no mark and restores in log order.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    ContentBlock, Delivery, DomainEvent, Envelope, MessageId, SteerOutcome, SteerResolved,
    ThreadId, TurnId, TurnQueued,
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
    /// A steer with no `SteerResolved` yet.
    unanswered: bool,
}

/// The queued turns of a log, in queue order, and the turn put back for the sign-in, if any.
struct QueuedTurns {
    turns: Vec<(Restored, MessageId, Vec<ContentBlock>)>,
    sign_in: Option<TurnId>,
}

/// A turn the restored queue holds.
enum Restored {
    /// A queued turn, as logged.
    Queued(TurnId),
    /// A steer never answered, sent into this turn: it gets a new turn now.
    Unanswered(TurnId),
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
    let mut turns = Vec::new();
    let queued = queued_turns(log);
    for (restored, message, content) in queued.turns {
        let turn = match restored {
            Restored::Queued(turn) => {
                if needs_queued(log, thread, turn) {
                    let waits = TurnQueued::new(thread, turn, thread, previous_turn(log, turn))
                        .map_err(|error| Error::Internal(error.to_string()))?;
                    out.push(actor, waits)?;
                }
                turn
            }
            Restored::Unanswered(steered) => {
                let turn: TurnId = actor.ids.mint(actor.now().wall)?;
                let resolved = SteerResolved {
                    message,
                    outcome: SteerOutcome::Requeued { turn },
                    aside: Vec::new(),
                };
                let waits = TurnQueued::new(thread, turn, thread, Some(steered))
                    .map_err(|error| Error::Internal(error.to_string()))?;
                out.push(actor, resolved)?;
                out.push(actor, waits)?;
                turn
            }
        };
        turns.push(Queued {
            turn,
            message,
            content,
            forced: false,
        });
    }
    let queue = &mut state(actor, thread).ok_or_else(|| lost("a thread"))?.queue;
    queue.turns = turns.into();
    queue.held = !queue.turns.is_empty();
    queue.sign_in = queued.sign_in;
    sync_held(actor, thread);
    Ok(())
}

/// The queued turns of `log` in queue order: turn, message, content as step 5 sends it. A turn
/// whose `TurnQueued` is marked `at_head` (a requeued steer, or a turn put back for the
/// sign-in) goes first, the latest put back first, as each took the head when it was put back;
/// the rest follow in the order they joined the queue.
fn queued_turns(log: &[Envelope]) -> QueuedTurns {
    let mut prompts: BTreeMap<MessageId, Prompt> = BTreeMap::new();
    let mut done: BTreeSet<TurnId> = BTreeSet::new();
    let mut heads: BTreeSet<TurnId> = BTreeSet::new();
    let mut steered: BTreeSet<TurnId> = BTreeSet::new();
    for (at, envelope) in log.iter().enumerate() {
        let event = &envelope.event;
        if let DomainEvent::UserMessage(message) = event {
            let prompt = Prompt {
                at,
                turn: message.turn,
                content: message.content.clone(),
                unanswered: message.delivery == Delivery::Steer,
            };
            prompts.insert(message.message, prompt);
        } else if let DomainEvent::QueuedPromptEdited(edited) = event
            && let Some(prompt) = prompts.get_mut(&edited.message)
        {
            prompt.content.clone_from(&edited.content);
        } else if let DomainEvent::SteerResolved(resolved) = event
            && let Some(prompt) = prompts.get_mut(&resolved.message)
        {
            prompt.unanswered = false;
            if let SteerOutcome::Requeued { turn } | SteerOutcome::SentNow { turn } =
                &resolved.outcome
            {
                prompt.turn = *turn;
                prompt.at = at;
            }
            if let SteerOutcome::Requeued { turn } = &resolved.outcome {
                steered.insert(*turn);
            }
        } else if let DomainEvent::TurnQueued(queued) = event
            && queued.is_at_head()
        {
            heads.insert(queued.turn());
        } else if let DomainEvent::TurnStarted(started) = event {
            done.insert(started.turn);
        } else if let DomainEvent::TurnEnded(ended) = event {
            done.insert(ended.turn);
        }
    }
    let mut queued: Vec<(Place, Restored, MessageId, Vec<ContentBlock>)> = prompts
        .into_iter()
        .filter_map(|(message, prompt)| {
            let (restored, head) = if prompt.unanswered {
                (Restored::Unanswered(prompt.turn), false)
            } else if done.contains(&prompt.turn) {
                return None;
            } else {
                (Restored::Queued(prompt.turn), heads.contains(&prompt.turn))
            };
            let place = if head {
                Place::Head(std::cmp::Reverse(prompt.at))
            } else {
                Place::Joined(prompt.at)
            };
            Some((place, restored, message, prompt.content))
        })
        .collect();
    queued.sort_by_key(|(place, ..)| *place);
    // A steer put back at the head is not waiting for the sign-in; the first turn put back for
    // it is.
    let sign_in = queued.iter().find_map(|(place, restored, ..)| {
        let Restored::Queued(turn) = restored else {
            return None;
        };
        (matches!(place, Place::Head(_)) && !steered.contains(turn)).then_some(*turn)
    });
    let turns = queued
        .into_iter()
        .map(|(_, restored, message, content)| (restored, message, content))
        .collect();
    QueuedTurns { turns, sign_in }
}

/// A restored turn's place: the turns put back at the head first, the latest first, then the
/// rest by where they joined the queue in the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Head(std::cmp::Reverse<usize>),
    Joined(usize),
}
