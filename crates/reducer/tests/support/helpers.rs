//! Helpers the reducer's test roots share: folding logs, applying deltas, reading changes.
//!
//! A test root includes it with `#[path = "support/helpers.rs"] pub mod helpers;` (`pub` keeps
//! a root that uses only some helpers free of unused warnings). Tests only: it unwraps.

use clauseo_protocol::{
    AppendTarget, Envelope, MessageId, Seq, TextKind, TurnId, ViewEntry, ViewWindow,
};
use clauseo_reducer::{
    Applied, Snapshot, StreamState, TextDelta, ViewChange, apply, apply_text_delta, view_snapshot,
};

/// The empty state of the log's stream.
pub fn empty(events: &[Envelope]) -> StreamState {
    StreamState::new(events.first().unwrap().stream)
}

/// Folds `events` into `state`, each accepted, and returns what each apply gave.
pub fn apply_all(state: &mut StreamState, events: &[Envelope]) -> Vec<Applied> {
    events
        .iter()
        .map(|envelope| apply(state, envelope).unwrap())
        .collect()
}

/// The state folded from `events`, which must not be empty.
pub fn fold(events: &[Envelope]) -> StreamState {
    let mut state = empty(events);
    apply_all(&mut state, events);
    state
}

/// The state, through snapshot bytes and back.
pub fn round_trip(state: &StreamState) -> StreamState {
    let bytes = Snapshot::of(state).to_bytes().unwrap();
    Snapshot::from_bytes(&bytes).unwrap().into_state().unwrap()
}

/// Applies a delta of `text` at the state's own `seq` as pending head.
pub fn delta(
    state: &mut StreamState,
    turn: TurnId,
    message: MessageId,
    kind: TextKind,
    text: &str,
) -> Applied {
    let pending_seq = state.seq().unwrap();
    delta_at(state, turn, message, kind, text, pending_seq)
}

/// Applies a delta of `text` with `pending_seq` as the host's pending head.
pub fn delta_at(
    state: &mut StreamState,
    turn: TurnId,
    message: MessageId,
    kind: TextKind,
    text: &str,
    pending_seq: Seq,
) -> Applied {
    let delta = TextDelta {
        turn,
        message,
        kind,
        text: text.to_owned(),
        pending_seq,
    };
    apply_text_delta(state.as_thread_mut().unwrap(), &delta).unwrap()
}

/// The `(target, from_len, text)` of each `AppendText` among `changes`.
pub fn appends(changes: &[ViewChange]) -> Vec<(AppendTarget, u32, String)> {
    changes
        .iter()
        .filter_map(|change| match change {
            ViewChange::AppendText {
                target,
                from_len,
                text,
                ..
            } => Some((*target, *from_len, text.clone())),
            ViewChange::Upsert(_) | ViewChange::Remove(_) => None,
        })
        .collect()
}

/// The entry of the first `Upsert` among `changes`.
pub fn first_upsert(changes: &[ViewChange]) -> ViewEntry {
    changes
        .iter()
        .find_map(|change| match change {
            ViewChange::Upsert(entry) => Some((**entry).clone()),
            ViewChange::AppendText { .. } | ViewChange::Remove(_) => None,
        })
        .unwrap()
}

/// The shown text of `message` in the view snapshot of `state`, if it has an entry.
pub fn shown_text(state: &StreamState, message: MessageId) -> Option<String> {
    agent_text(&view_snapshot(state), message)
}

/// The text of `message`'s agent text entry in `window`.
pub fn agent_text(window: &ViewWindow, message: MessageId) -> Option<String> {
    window.entries.iter().find_map(|entry| match entry {
        ViewEntry::AgentText(text) if text.message == message => Some(text.text.clone()),
        _ => None,
    })
}

/// The `n`th envelope's `Seq`.
pub fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}
