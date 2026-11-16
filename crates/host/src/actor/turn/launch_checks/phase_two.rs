//! Phase two of a launch ([`on_prepared`]): the checks answered, and the agent launches with
//! what they found, or the waiting work fails. Moved out of `launch_checks.rs` unchanged.

use std::sync::Arc;

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::{agents, host_stopping, state};
use super::{Prepared, browser, discard, packs};
use crate::error::Error;
use crate::log_file::LIFECYCLE;

/// Phase two: the checks `id` answered. The agent launches with what they found, or the
/// waiting work fails with their error; during shutdown it fails as closed. On every way that
/// does not end in a launched agent, the launch's run folder is removed off the actor; an answer
/// for an earlier launch removes only its own.
pub(in crate::actor::turn) fn on_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Box<Prepared>, Error>,
) {
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        if let Ok(stale) = result {
            discard(actor, thread, stale.run, None);
        }
        return;
    };
    let waiting = preparing.waiting;
    tracing::info!(target: LIFECYCLE, %thread, ok = result.is_ok(), "launch prep done");
    remember_folder_support(actor, thread, &result);
    let mut prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    if host_stopping(actor) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    browser::warn_if_still_full(actor, &prepared.run);
    // A refusal that came, or stayed, while the checks ran (a key refusal waited out
    // included): nothing launches.
    if let Some(error) = super::super::sign_in::launch_refusal(actor, thread) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::super::sign_in::check_launch(actor, thread, &prepared) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    if let Err(error) = super::super::profile_loads::launch_keys(actor, thread, &mut prepared) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    prepared.run.lawyer = Arc::new(super::super::profile_loads::lawyer_servers(
        actor,
        &prepared.run,
    ));
    let run = prepared.run.clone();
    let packs_skipped = std::mem::take(&mut prepared.packs_skipped);
    if let Some(state) = state(actor, thread) {
        state.run = Some(run.clone());
    }
    let failed_tree = agents::launch_now(actor, thread, waiting, *prepared);
    let launched = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if launched {
        packs::hold_notice(actor, thread, packs_skipped);
    } else {
        if let Some(state) = state(actor, thread) {
            state.run = None;
        }
        // A process that was started and then failed is waited for like any other.
        discard(actor, thread, run, failed_tree);
    }
}

/// A launch's folder check answered (`agent/folder_support.rs`, a Codex or a Claude agent):
/// its answer goes to the matter's row, and so does a refusal it made. Any other agent, and a
/// launch that failed before or without the check's refusal, leaves the row as it was.
fn remember_folder_support(
    actor: &mut Actor,
    thread: ThreadId,
    result: &Result<Box<Prepared>, Error>,
) {
    let support = match result {
        Ok(prepared) => match prepared.folder {
            Some(answer) => answer.row_kind(),
            None => return,
        },
        Err(error) => match crate::agent::folder_support::of_refusal(error.code()) {
            Some(kind) => Some(kind),
            None => return,
        },
    };
    let Some(matter) = actor.threads.get(&thread).map(|entry| entry.row.matter) else {
        return;
    };
    crate::actor::query::on_folder_support(actor, matter, support);
}
