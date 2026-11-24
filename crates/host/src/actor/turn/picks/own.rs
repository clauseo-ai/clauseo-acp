//! A thread's own model and effort (`ThreadRow::own_picks`): what the lawyer last chose in it,
//! kept apart from what a session ran with.
//!
//! - A pick that the agent takes becomes the kept value of its option ([`keep`]): a
//!   `SetConfigOption` (answered at once, or waiting for the session and covered by its start),
//!   a prompt's own pick (`prompt_picks/send.rs`), and `CreateThread`'s config once a session
//!   started with it. A pick made while the session runs on a fallback list counts too.
//! - A drop ("not listed" or "refused") never touches them, and leaves them in the matter's
//!   prefs (`picks::after`), so each session start tries them again and a list that has them
//!   again gets them back without a notice.
//! - A thread from before schema 9 has none: its first session start in this build takes them
//!   from the thread's logged options ([`seed`]), which the attach commit then writes.

use clauseo_protocol::{ConfigOption, ConfigPicks, ThreadId};
use clauseo_store::ThreadRow;

use super::{Offer, Pick, current_pick, logged, offer};
use crate::actor::Actor;

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

/// `CreateThread`'s config, once the start's `options` hold its value: the thread's first
/// session started with it, so it is the thread's own. The attach commit writes the row.
pub(super) fn keep_created(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) {
    // The first session only: a later one may run a fallback that equals the old config.
    let Some(created) = actor
        .threads
        .get(&thread)
        .filter(|entry| entry.row.acp_session_id.is_none())
        .and_then(|entry| entry.picks.as_ref())
    else {
        return;
    };
    let held: Vec<Pick> = pairs(created)
        .into_iter()
        .filter(|pick| offer(options, pick) == Offer::Held)
        .collect();
    keep(actor, thread, &held);
}

/// Whether `pick` is the thread's kept value of its option: a drop never takes it out of the
/// matter's prefs or `CreateThread`'s config.
pub(super) fn is_kept(own: &[Pick], pick: &Pick) -> bool {
    own.contains(pick)
}
