//! The note read from the logs, so a restart tells the agent what it had not heard: the thread's
//! undos since its last turn that carried the note, and where the matter's log is read from for
//! the restores since then.
//!
//! A turn carried the note when the lawyer's prompt opened it and the agent took that prompt:
//! it said or did something in it (text, a non-text block, a tool call, a permission request
//! or a question), or stopped of its own accord. A prompt refused or failed before the agent
//! took it, a lawyer's cancel before it did anything, or a turn the agent started with no
//! prompt, leaves the note for the next prompt.

use std::collections::HashSet;

use clauseo_protocol::{
    CheckpointId, DomainEvent, Envelope, MarkerKind, StopReason, Timestamp, TurnId, ViewEntry,
};

use super::{NoteInput, RestoreNote};

/// The note as the thread's log tells it.
pub(crate) struct Read<'a> {
    /// The undos, with a `TurnStarted` for each turn that carried the note, in log order.
    pub(crate) inputs: Vec<NoteInput<'a>>,
    /// The after checkpoint of the last turn that carried the note: the matter's restores
    /// logged after its `CheckpointRecorded` are the note's too ([`Floor::After`]). `None` when
    /// no turn carried it, or that turn has no after checkpoint (it failed).
    pub(crate) since: Option<CheckpointId>,
    /// When that turn ended: without its checkpoint (or the store's row of it), the restores
    /// logged since are the note's ([`Floor::At`]). `None` when no turn carried the note: then
    /// those since the conversation was created are.
    pub(crate) ended: Option<Timestamp>,
}

/// Where the note's restores start in the matter's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Floor {
    /// After the `CheckpointRecorded` of the last turn that carried the note: the log is read
    /// from there, and a restore whose checkpoint it is came before that turn.
    After(CheckpointId),
    /// Logged at this time or later: the end of that turn, or the conversation's creation when
    /// no turn carried the note. Nothing logged since was told yet.
    At(Timestamp),
}

/// The note's inputs from the thread's view entries, in log order, in one pass.
pub(crate) fn read<'a>(entries: impl Iterator<Item = &'a ViewEntry>) -> Read<'a> {
    let mut prompted = HashSet::new();
    let mut acted = HashSet::new();
    let mut asked = false;
    let mut inputs = Vec::new();
    let (mut since, mut ended) = (None, None);
    for entry in entries {
        if let ViewEntry::UserMessage(message) = entry {
            prompted.insert(message.turn);
        } else if let Some(turn) = spoken(entry) {
            acted.insert(turn);
        } else if asks(entry) {
            asked = true;
        } else if let ViewEntry::Step(step) = entry {
            let took = asked || answered(step.stop) || acted.contains(&step.turn);
            asked = false;
            if prompted.contains(&step.turn) && took {
                inputs.push(NoteInput::TurnStarted);
                since = step.checkpoint_after;
                ended = Some(step.ts);
            }
        } else if let ViewEntry::Marker(marker) = entry
            && let MarkerKind::StepUndone { restored, .. } = &marker.kind
        {
            inputs.push(NoteInput::Undone {
                restored,
                at: marker.ts,
            });
        }
    }
    Read {
        inputs,
        since,
        ended,
    }
}

/// Whether `turn`, whose prompt carried the note, reached the agent: read backwards from the
/// log's end through that turn's own entries only, which come between its step and the step
/// before it. A turn whose step is not in `entries` did not.
pub(crate) fn carried<'a>(
    entries: impl DoubleEndedIterator<Item = &'a ViewEntry>,
    turn: TurnId,
) -> bool {
    let mut inside = false;
    for entry in entries.rev() {
        if let ViewEntry::Step(step) = entry {
            if inside {
                return false;
            }
            if step.turn == turn {
                if answered(step.stop) {
                    return true;
                }
                inside = true;
            }
        } else if inside && (asks(entry) || spoken(entry) == Some(turn)) {
            return true;
        }
    }
    false
}

/// Whether `envelope`, read from the matter's log at `floor` or after, is a restore the note
/// tells.
pub(crate) fn kept(envelope: &Envelope, floor: Floor) -> bool {
    let DomainEvent::VersionRestored(restored) = &envelope.event else {
        return false;
    };
    match floor {
        Floor::After(since) => restored.checkpoint != since,
        Floor::At(at) => envelope.ts >= at,
    }
}

/// The restores in `envelopes` (those [`kept`] keeps), each as the note tells it: one that wrote
/// or removed no file it names is left out.
pub(crate) fn restores_in(envelopes: &[Envelope]) -> Vec<RestoreNote> {
    envelopes
        .iter()
        .filter_map(|envelope| {
            let DomainEvent::VersionRestored(restored) = &envelope.event else {
                return None;
            };
            let more = restored.more.unwrap_or_default();
            let names = !restored.written.is_empty() || !restored.removed.is_empty() || more > 0;
            names.then(|| RestoreNote {
                back: restored.written.clone(),
                removed: restored.removed.clone(),
                more,
                checkpoint: Some(restored.checkpoint),
                at: envelope.ts,
            })
        })
        .collect()
}

/// Whether a turn that stopped so stopped of the agent's own accord, so the prompt reached it.
const fn answered(stop: StopReason) -> bool {
    matches!(
        stop,
        StopReason::EndTurn
            | StopReason::MaxTokens
            | StopReason::MaxTurnRequests
            | StopReason::Refusal
    )
}

/// The turn of an entry the agent said or did: text, a non-text block or a tool call.
fn spoken(entry: &ViewEntry) -> Option<TurnId> {
    if let ViewEntry::AgentText(text) = entry {
        Some(text.turn)
    } else if let ViewEntry::AgentContent(content) = entry {
        Some(content.turn)
    } else if let ViewEntry::ToolCall(call) = entry {
        Some(call.turn)
    } else {
        None
    }
}

/// Whether the agent asked the lawyer something: a permission request or a question. These name
/// no turn; each belongs to the turn whose step comes next.
const fn asks(entry: &ViewEntry) -> bool {
    matches!(entry, ViewEntry::Permission(_) | ViewEntry::Elicitation(_))
}

#[cfg(test)]
mod tests;
