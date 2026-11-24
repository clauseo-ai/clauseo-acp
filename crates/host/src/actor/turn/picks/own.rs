//! A thread's own model and effort (`ThreadRow::own_picks`): what the lawyer last chose in it,
//! kept apart from what a session ran with.
//!
//! - A pick that the agent takes becomes the kept value of its option ([`keep`]): a
//!   `SetConfigOption` (answered at once, or waiting for the session and covered by its start),
//!   a prompt's own pick (`prompt_picks/send.rs`), and `CreateThread`'s config (else the
//!   matter's prefs) once the thread's first session started with it. A pick made while the
//!   session runs on a fallback list counts too.
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
//! - A thread from before schema 9 has none (`NULL`): its first session start in this build
//!   takes them from the thread's logged options ([`seed`]), which the attach commit then
//!   writes. A thread made in this build starts with an empty set; its first session seeds what
//!   it runs with ([`seed_first`]), what the lawyer saw in the header at its first answer.

use clauseo_acp::AgentUpdate;
use clauseo_protocol::{AgentKind, ConfigCategory, ConfigOption, ConfigPicks, ThreadId};
use clauseo_store::ThreadRow;

use super::{Offer, Pick, current_pick, logged, offer, remembered};
use crate::actor::Actor;
use crate::log_file::LIFECYCLE;

/// The thread's kept values: its row's, else (a thread not seeded yet) its logged ones.
pub(in crate::actor::turn) fn own(actor: &Actor, thread: ThreadId) -> Vec<Pick> {
    if let Some(picks) = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.row.own_picks.as_ref())
    {
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

/// A session of `thread` starts: a thread with no kept values yet takes its logged ones, before
/// the new session logs options of its own. The attach commit writes the row.
pub(in crate::actor::turn) fn seed(actor: &mut Actor, thread: ThreadId) {
    let seeded = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.row.own_picks.is_some());
    if seeded {
        return;
    }
    let picks: Vec<Pick> = logged(actor, thread)
        .map(|own| own.iter().filter_map(current_pick).collect())
        .unwrap_or_default();
    if picks.is_empty() {
        return;
    }
    let Ok(picks) = ConfigPicks::new(picks.into_iter().collect()) else {
        return;
    };
    if let Some(entry) = actor.threads.get_mut(&thread) {
        entry.row.own_picks = Some(picks);
    }
}

/// The agent took `picks` in `thread`'s session: they become its kept values. The thread row
/// to write, set on the host's row now; `None` when the row held them already.
pub(in crate::actor::turn) fn keep(
    actor: &mut Actor,
    thread: ThreadId,
    picks: &[Pick],
) -> Option<ThreadRow> {
    let entry = actor.threads.get_mut(&thread)?;
    let mut own = entry.row.own_picks.clone().unwrap_or_default();
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
    entry.row.own_picks = Some(own);
    Some(entry.row.clone())
}

/// `CreateThread`'s config, else the matter's prefs, once the start's `options` hold its value:
/// the thread's first session started with the lawyer's choice, so it is the thread's own. The
/// attach commit writes the row.
pub(super) fn keep_created(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) {
    // The first session only: a later one may run a fallback that equals the old config.
    let first = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.row.acp_session_id.is_none());
    let Some(created) = remembered(actor, thread).filter(|_| first) else {
        return;
    };
    let held: Vec<Pick> = pairs(created)
        .into_iter()
        .filter(|pick| offer(options, pick) == Offer::Held)
        .collect();
    keep(actor, thread, &held);
}

/// The thread's first session started with `options`, and the thread keeps nothing yet (no pick
/// of the lawyer's, no config or prefs it held): the model and effort it runs with, which the
/// lawyer saw in the header, become the kept values. The attach commit writes the row.
pub(in crate::actor::turn) fn seed_first(
    actor: &mut Actor,
    thread: ThreadId,
    options: &[ConfigOption],
) {
    let empty_first = actor.threads.get(&thread).is_some_and(|entry| {
        entry.row.acp_session_id.is_none()
            && entry
                .row
                .own_picks
                .as_ref()
                .is_some_and(ConfigPicks::is_empty)
    });
    if !empty_first {
        return;
    }
    let picks: Vec<Pick> = options.iter().filter_map(current_pick).collect();
    keep(actor, thread, &picks);
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
