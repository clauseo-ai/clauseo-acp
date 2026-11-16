//! The instruction parts a Codex agent was last told, by part (`ToldOnce::parts`), and the ones
//! that changed since: Codex keeps a resumed thread's first instructions (codex 0.162.1 injects
//! `developer_instructions` only as a new thread's full context, `core/src/session/mod.rs`
//! `record_context_updates_and_set_reference_context_item`), so a part that changed goes, as a
//! note, first in the lawyer's next message (`note.rs`), never shown to the lawyer.
//!
//! A new session knows each part as its instructions give it ([`baseline`]): for the other
//! matters, none, as Codex's instructions name no matter; so a new session with invites hears
//! them at its first prompt and a fresh thread with none hears nothing. After a start of the app
//! nothing is known of what a resumed thread was told, so each part goes once more on its first
//! prompt. One part for now, the other matters this conversation may read ([`MATTERS`]); another
//! part is one more line in [`current`] and [`baseline`].

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::state;

/// The other matters this conversation may read (`outside::fence::codex_reads_note`).
const MATTERS: &str = "matters";

/// Each part as the agent should know it now, by key, in the order they go: none but for a
/// Codex agent with its run.
fn current(actor: &mut Actor, thread: ThreadId) -> Vec<(&'static str, String)> {
    let Some(turns) = state(actor, thread) else {
        return Vec::new();
    };
    let codex = turns
        .agent
        .as_deref()
        .is_some_and(|agent| agent.codex.is_some());
    let Some(run) = turns.run.as_ref().filter(|_| codex) else {
        return Vec::new();
    };
    vec![(MATTERS, super::super::outside::fence::codex_reads_note(run))]
}

/// What a new session's instructions tell of part `key`.
fn baseline(key: &'static str) -> String {
    match key {
        MATTERS => clauseo_profile::instructions::reads_note(&[]),
        _ => String::new(),
    }
}

/// The parts that changed since the agent was last told them, by key, in order.
pub(super) fn changed(actor: &mut Actor, thread: ThreadId) -> Vec<(&'static str, String)> {
    let now = current(actor, thread);
    let Some(turns) = state(actor, thread) else {
        return Vec::new();
    };
    now.into_iter()
        .filter(|(key, text)| turns.told_once.parts.get(key) != Some(text))
        .collect()
}

/// A session is up: a new one (not `loaded`) knows each part as its instructions give it
/// ([`baseline`]).
pub(in crate::actor::turn) fn session_started(actor: &mut Actor, thread: ThreadId, loaded: bool) {
    if loaded {
        return;
    }
    let known: Vec<(&'static str, String)> = current(actor, thread)
        .into_iter()
        .map(|(key, _)| (key, baseline(key)))
        .collect();
    if let Some(turns) = state(actor, thread) {
        turns.told_once.parts.extend(known);
    }
}

/// Records `parts` as told, and gives back what each was before (`None`: never told).
pub(super) fn told(
    actor: &mut Actor,
    thread: ThreadId,
    parts: Vec<(&'static str, String)>,
) -> Vec<(&'static str, Option<String>)> {
    let Some(turns) = state(actor, thread) else {
        return Vec::new();
    };
    parts
        .into_iter()
        .map(|(key, text)| (key, turns.told_once.parts.insert(key, text)))
        .collect()
}

/// Puts back what `before` held: the prompt that told those parts did not reach the agent.
pub(super) fn untold(
    actor: &mut Actor,
    thread: ThreadId,
    before: Vec<(&'static str, Option<String>)>,
) {
    let Some(turns) = state(actor, thread) else {
        return;
    };
    for (key, text) in before {
        match text {
            Some(text) => turns.told_once.parts.insert(key, text),
            None => turns.told_once.parts.remove(key),
        };
    }
}
