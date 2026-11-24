//! A thread's own model and effort (`ThreadRow::own_picks`): what the lawyer last chose in it,
//! kept apart from what a session ran with, per agent.
//!
//! - Per agent: the row keeps an entry for each agent the thread ran on, and every read and
//!   write here takes the entry of the thread's agent now (`ThreadRow::agent`), so one agent's
//!   model id is never set on another. A thread whose agent changed starts with the new agent's
//!   entry (none the first time) and gets the first agent's back when it changes back.
//! - A pick that the agent takes becomes the kept value of its option ([`keep`]): a
//!   `SetConfigOption` (answered at once, or waiting for the session and covered by its start)
//!   and a prompt's own pick (`prompt_picks/send.rs`). A pick made while the session runs on a
//!   fallback list counts too.
//! - `CreateThread`'s config (else the matter's prefs) is kept at the agent's first session
//!   ([`keep_created`]) when the session holds it, or, for a Codex model, does not list it
//!   (Codex's fallback list: the lawyer's choice stays, and the start tells it was not used).
//!   One the agent refused is not, nor an effort or another agent's value it does not list.
//!   What the first session runs with fills the options still kept for nothing
//!   ([`seed_first`]), what the lawyer saw in the header at its first answer.
//! - A drop ("not listed" or "refused") never touches them, and leaves them in the matter's
//!   prefs (`picks::after`), so each session start tries them again and a list that has them
//!   again gets them back without a notice.
//! - codex-acp's warning that a resumed session runs on another model than it was recorded with
//!   is left out for a thread that keeps a model ([`drops_resume_warning`]): the start sets the
//!   kept model back, or Clauseo's own notice says it could not, so the lawyer gets one notice
//!   at most, Clauseo's.
//! - [`keep`] sets the host's row before its commit is durable, as `picks::remember` does for
//!   the matter's prefs: when that commit cannot be sent, the next write of the row carries the
//!   value without its event (a known limit).
//! - A thread from before schema 9 has none (`NULL`), nor does a thread whose stored value does
//!   not decode (the column is a hint): its next session start takes its agent's entry from what
//!   its last session logged ([`seed`]), which the attach commit then writes. So a thread from
//!   before the migration seeds from what its last session logged, and one already hit by a
//!   fallback keeps that fallback until the lawyer picks again.

use clauseo_acp::AgentUpdate;
use clauseo_protocol::{
    AgentChoiceId, AgentKind, ConfigCategory, ConfigOption, ConfigPicks, ThreadId,
};
use clauseo_store::ThreadRow;

use super::{Offer, Pick, Picked, current_pick, logged, offer, remembered};
use crate::actor::Actor;
use crate::agent::forced_ask::config_allowed;
use crate::log_file::LIFECYCLE;

/// The kept values of the thread's agent now, once it has an entry.
fn kept_of(row: &ThreadRow) -> Option<&ConfigPicks> {
    row.own_picks.as_ref()?.of(&row.agent)
}

/// The thread's kept values for its agent: its row's entry, else (not seeded yet) its logged
/// ones.
pub(in crate::actor::turn) fn own(actor: &Actor, thread: ThreadId) -> Vec<Pick> {
    if let Some(picks) = (actor.threads.get(&thread)).and_then(|entry| kept_of(&entry.row)) {
        return pairs(picks);
    }
    logged(actor, thread)
        .map(|own| own.iter().filter_map(current_pick).collect())
        .unwrap_or_default()
}

/// The picks of `picks`, in option order.
pub(super) fn pairs(picks: &ConfigPicks) -> Vec<Pick> {
    let pairs = picks.as_map().iter();
    pairs
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

/// A session of `thread` starts: a thread with no kept values for its agent yet takes its
/// logged ones, before the new session logs options of its own. The attach commit writes the
/// row.
pub(in crate::actor::turn) fn seed(actor: &mut Actor, thread: ThreadId) {
    let seeded = (actor.threads.get(&thread)).is_some_and(|entry| kept_of(&entry.row).is_some());
    if seeded {
        return;
    }
    let picks: Vec<Pick> = logged(actor, thread)
        .map(|own| own.iter().filter_map(current_pick).collect())
        .unwrap_or_default();
    if !picks.is_empty() {
        keep(actor, thread, &picks);
    }
}

/// The agent took `picks` in `thread`'s session: they become its agent's kept values. The
/// thread row to write, set on the host's row now; `None` when the row held them already.
pub(in crate::actor::turn) fn keep(
    actor: &mut Actor,
    thread: ThreadId,
    picks: &[Pick],
) -> Option<ThreadRow> {
    let row = &mut actor.threads.get_mut(&thread)?.row;
    let mut all = row.own_picks.clone().unwrap_or_default();
    let own = all.0.entry(row.agent.clone()).or_default();
    let mut changed = false;
    for (option, value) in picks {
        if own.as_map().get(option) != Some(value) && own.set(option.clone(), value.clone()).is_ok()
        {
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    row.own_picks = Some(all);
    Some(row.clone())
}

/// Whether `thread`'s session is the first of its agent: no session of it has attached yet
/// (a new thread, or one whose agent changed).
fn first(actor: &Actor, thread: ThreadId) -> bool {
    (actor.threads.get(&thread)).is_some_and(|entry| entry.row.acp_session_id.is_none())
}

/// The options of the thread's agent that keep no value yet.
fn unkept(actor: &Actor, thread: ThreadId, picks: impl IntoIterator<Item = Pick>) -> Vec<Pick> {
    let own = (actor.threads.get(&thread)).and_then(|entry| kept_of(&entry.row));
    let kept = |option: &_| own.is_some_and(|own| own.as_map().contains_key(option));
    picks
        .into_iter()
        .filter(|(option, _)| !kept(option))
        .collect()
}

/// `CreateThread`'s config, else the matter's prefs, at the first session of the thread's
/// agent, for the options it keeps nothing for: a value the start's `options` hold, or a Codex
/// model `picked` dropped as not listed (a fallback list), is the lawyer's choice, so the
/// thread's own; the start tells the second was not used. One the agent refused is not kept.
/// The attach commit writes the row.
pub(super) fn keep_created(
    actor: &mut Actor,
    thread: ThreadId,
    options: &[ConfigOption],
    picked: &Picked,
) {
    // The first session only: a later one may run a fallback that equals the old config.
    let Some(created) = remembered(actor, thread).filter(|_| first(actor, thread)) else {
        return;
    };
    // Dropped as not listed: a Codex model Clauseo may set, which the list lacks (Codex's
    // fallback list). An effort follows the model, and another agent's list is no fallback, so
    // those go as before (a retired value too).
    let codex = (actor.threads.get(&thread))
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex));
    let model = |id: &AgentChoiceId| {
        (options.iter())
            .any(|option| option.id == id.as_str() && option.category == ConfigCategory::Model)
    };
    let not_listed = |pick: &Pick| {
        codex
            && picked.stale.contains(pick)
            && !picked.refused.contains(pick)
            && model(&pick.0)
            && config_allowed(pick.0.as_str(), Some(pick.1.as_str()))
    };
    let created: Vec<Pick> = pairs(created)
        .into_iter()
        .filter(|pick| offer(options, pick) == Offer::Held || not_listed(pick))
        .collect();
    let created = unkept(actor, thread, created);
    keep(actor, thread, &created);
}

/// The first session of the thread's agent started with `options`: the model and effort it runs
/// with, which the lawyer saw in the header, become the kept values of the options it keeps
/// nothing for (no pick of the lawyer's, no config or prefs kept). The attach commit writes the
/// row.
pub(in crate::actor::turn) fn seed_first(
    actor: &mut Actor,
    thread: ThreadId,
    options: &[ConfigOption],
) {
    if !first(actor, thread) {
        return;
    }
    let running = unkept(actor, thread, options.iter().filter_map(current_pick));
    keep(actor, thread, &running);
}

/// Whether `pick` is the thread's kept value of its option: a drop never takes it out of the
/// matter's prefs or `CreateThread`'s config.
pub(super) fn is_kept(own: &[Pick], pick: &Pick) -> bool {
    own.contains(pick)
}

/// Whether `update`, outside a turn of Codex's `thread`, is codex-acp's warning that the
/// resumed session runs on another model than it was recorded with
/// (`clauseo_acp::codex_resumed_model`), and the thread keeps a model: it is left out, since the
/// start's picks set the kept model back, or Clauseo's own notice says they could not. Any other
/// notice, and this one for a thread that keeps no model, shows.
pub(in crate::actor::turn) fn drops_resume_warning(
    actor: &Actor,
    thread: ThreadId,
    update: &AgentUpdate,
) -> bool {
    let AgentUpdate::Notice { text, .. } = update else {
        return false;
    };
    let Some((recorded, resumed)) = clauseo_acp::codex_resumed_model(text) else {
        return false;
    };
    let codex = (actor.threads.get(&thread))
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex));
    let options = super::thread_options(actor, thread);
    let model = |id: &str| {
        (options.iter()).any(|option| option.id == id && option.category == ConfigCategory::Model)
    };
    if !codex || !own(actor, thread).iter().any(|(id, _)| model(id.as_str())) {
        return false;
    }
    tracing::info!(target: LIFECYCLE, %thread, recorded, resumed, "codex-acp's resume warning left out: the kept model is set back or told");
    true
}
