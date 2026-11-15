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
    CheckpointId, DomainEvent, Envelope, MarkerKind, StopReason, TurnId, ViewEntry,
};

use super::{NoteInput, RestoreNote};

/// The note as the thread's log tells it.
pub(crate) struct Read<'a> {
    /// The undos, with a `TurnStarted` for each turn that carried the note, in log order.
    pub(crate) inputs: Vec<NoteInput<'a>>,
    /// The after checkpoint of the last turn that carried the note: the matter's restores
    /// logged after its `CheckpointRecorded` are the note's too (`restores_after`). `None`
    /// when no turn carried it, or that turn has no after checkpoint (it failed): the agent then
    /// hears of no restore from the log, only of those memory holds.
    pub(crate) since: Option<CheckpointId>,
}

/// The note's inputs from the thread's view entries, in log order, in one pass.
pub(crate) fn read<'a>(entries: impl Iterator<Item = &'a ViewEntry>) -> Read<'a> {
    let mut prompted = HashSet::new();
    let mut acted = HashSet::new();
    let mut asked = false;
    let mut inputs = Vec::new();
    let mut since = None;
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
    Read { inputs, since }
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

/// The matter's restores in `envelopes`, the matter's log after the `CheckpointRecorded` of
/// `since`, each as the note tells it. A restore whose checkpoint is `since` itself came before
/// that turn (it reused the restore's checkpoint) and was told then.
pub(crate) fn restores_after(envelopes: &[Envelope], since: CheckpointId) -> Vec<RestoreNote> {
    envelopes
        .iter()
        .filter_map(|envelope| {
            let DomainEvent::VersionRestored(restored) = &envelope.event else {
                return None;
            };
            let more = restored.more.unwrap_or_default();
            let names = !restored.written.is_empty() || !restored.removed.is_empty() || more > 0;
            (names && restored.checkpoint != since).then(|| RestoreNote {
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
