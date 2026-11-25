//! A thread's own model and effort (`ThreadRow::own_picks`): what the lawyer last chose in it,
//! kept apart from what a session ran with, per agent.
//!
//! - Per agent: the row keeps an entry for each agent the thread ran on, and every read and
//!   write here takes the entry of the thread's agent now (`ThreadRow::agent`), so one agent's
//!   model id is never set on another. A thread whose agent changed starts with the new agent's
//!   entry (none the first time) and gets the first agent's back when it changes back.
//! - A pick that the agent takes becomes the kept value of its option ([`keep`],
//!   [`keep_answer`]): a `SetConfigOption` (answered at once, or waiting for the session and
//!   covered by its start), and with a model pick the effort its answer runs with, since the
//!   efforts on offer follow the model (an effort pick never keeps the model it runs on, which
//!   may be a fallback); a prompt's own pick (`prompt_picks/send.rs`) keeps its values. A pick
//!   made while the session runs on a fallback list counts too.
//! - `CreateThread`'s config (else the matter's prefs) is kept at the agent's first session
//!   ([`keep_created`]) when the session holds it, or, for a Codex model, does not list it
//!   (Codex's fallback list: the lawyer's choice stays, and the start tells it was not used),
//!   with the effort chosen beside that model when the fallback model lacks it too. One the
//!   agent refused is not, nor another agent's value it does not list: on Claude "not listed"
//!   is final, so a model an account gains later has to be picked again.
//!   What the first session runs with fills the options still kept for nothing
//!   ([`seed_first`]), what the lawyer saw in the header at its first answer.
//! - A drop ("not listed" or "refused") never touches them, and leaves them in the matter's
//!   prefs (`picks::after`), so each session start tries them again and a list that has them
//!   again gets them back without a notice. A start that fails to set one back, for whatever
//!   reason, tells it once (`pick_not_used`). One exception: a kept effort the kept model, running,
//!   does not list is no longer kept ([`forget_unoffered_efforts`]); it runs at the model's
//!   default with no notice.
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

/// The lawyer's `pick` was taken and `options` are the agent's answer: the pick becomes the
/// kept value of its option. A model pick keeps the effort the answer runs with too, since the
/// efforts on offer follow the model and the agent may have changed it with the model; an
/// effort pick keeps nothing else, so a fallback model it runs on never replaces the kept one.
pub(in crate::actor::turn) fn keep_answer(
    actor: &mut Actor,
    thread: ThreadId,
    pick: &Pick,
    options: &[ConfigOption],
) -> Option<ThreadRow> {
    let category = |category| {
        (options.iter()).any(|option| option.id == pick.0.as_str() && option.category == category)
    };
    let mut picks = vec![pick.clone()];
    if category(ConfigCategory::Model) {
        let efforts = options
            .iter()
            .filter(|option| option.category == ConfigCategory::ThoughtLevel);
        picks.extend(efforts.filter_map(current_pick));
    }
    keep(actor, thread, &picks)
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
    // fallback list), and with it an effort the fallback model lacks, which the lawyer chose for
    // that model. Another agent's list is no fallback, so its values go as before (a retired
    // value too), and so does an effort beside a model the list has.
    let codex = (actor.threads.get(&thread))
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex));
    let of = |id: &AgentChoiceId, category| {
        (options.iter()).any(|option| option.id == id.as_str() && option.category == category)
    };
    let dropped = |pick: &Pick| {
        codex
            && picked.stale.contains(pick)
            && !picked.refused.contains(pick)
            && config_allowed(pick.0.as_str(), Some(pick.1.as_str()))
    };
    let created = pairs(created);
    let model_gone =
        (created.iter()).any(|pick| of(&pick.0, ConfigCategory::Model) && dropped(pick));
    let not_listed = |pick: &Pick| {
        dropped(pick)
            && (of(&pick.0, ConfigCategory::Model)
                || (model_gone && of(&pick.0, ConfigCategory::ThoughtLevel)))
    };
    let created: Vec<Pick> = created
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

/// The kept model runs (set by the start, or held already) and `picked` dropped a kept effort as
/// not listed against that model's list: the model does not offer it, so it is no longer kept,
/// and leaves the prefs and the config as any dropped pick does (`picks::after`). The effort
/// runs at the model's default with no notice; a refused effort stays (it is told). The attach
/// commit writes the row.
pub(super) fn forget_unoffered_efforts(
    actor: &mut Actor,
    thread: ThreadId,
    picked: &Picked,
    options: &[ConfigOption],
) {
    let of = |id: &AgentChoiceId, category| {
        (options.iter()).any(|option| option.id == id.as_str() && option.category == category)
    };
    let Some(row) = actor.threads.get_mut(&thread).map(|entry| &mut entry.row) else {
        return;
    };
    let agent = row.agent.clone();
    let Some(own) = row.own_picks.as_mut().and_then(|all| all.0.get_mut(&agent)) else {
        return;
    };
    let kept = pairs(own);
    let model_runs = (kept.iter())
        .any(|pick| of(&pick.0, ConfigCategory::Model) && offer(options, pick) == Offer::Held);
    if !model_runs {
        return;
    }
    for pick in &kept {
        let unoffered = of(&pick.0, ConfigCategory::ThoughtLevel)
            && picked.stale.contains(pick)
            && !picked.refused.contains(pick);
        if unoffered && own.remove(pick.0.as_str()) {
            tracing::info!(target: LIFECYCLE, %thread, option = %pick.0, "a kept effort the kept model does not offer is no longer kept");
        }
    }
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
