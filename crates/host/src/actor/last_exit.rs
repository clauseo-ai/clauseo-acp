//! `LastExit`: how the run before this one ended, for the room's notice after Clauseo closed
//! unexpectedly. Whether that run quit through its own quit is the desktop's to say
//! (`HostConfig::with_last_run_unclean`); which file actions it left unfinished is the store's.
//!
//! An undo, a restore and a tracked-changes decision each hold their matter's writer and write
//! files one by one, so a crash part way leaves some old and some new. Before one is sent to the
//! matter's thread, [`record`] puts its row in the store's `pending_actions` and waits until it
//! is durable; when the writer passes on (`end::pass_writer`), [`finished`] deletes it. A put for
//! a matter whose delete is still on its way waits for that delete, so the two never cross.
//! Start-up reads what is left ([`at_start`]), which are the actions the last run did not finish,
//! keeps them for this run's answer, and deletes them: many clean quits never stop the host (an
//! idle quit, a logoff, a wait that ran out), so no later point is sure to come. A second crash
//! before the notice is seen loses the names; the desktop's flag still shows the notice. Logs
//! carry ids and counts only.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{InterruptedAction, LastExit, MatterId, QueryResult, ThreadId, Timestamp};
use clauseo_store::{PendingAction, Store};

use super::Actor;
use super::input::{Input, Reply};
use super::streams::Waiter;
use crate::log_file::{Cause, LIFECYCLE};

/// The actor's part: this run's answer, the matters whose writer holds a recorded action, what
/// waits for each record's put, the matters whose record's delete is on its way, and the records
/// held back behind such a delete. One action holds a matter's writer at a time, so one put per
/// matter at most waits.
pub(crate) struct LastExitHost {
    view: LastExit,
    recorded: BTreeSet<MatterId>,
    waiting: BTreeMap<MatterId, Waiter>,
    clearing: BTreeSet<MatterId>,
    held: BTreeMap<MatterId, PendingAction>,
}

/// A record's put or clear came back. Plain data, so it posts from the store's reply like every
/// other input; what runs next waits in [`LastExitHost`].
#[derive(Debug)]
pub(crate) struct Recorded {
    result: Result<(), clauseo_store::Error>,
    matter: MatterId,
    /// A put, whose waiter runs now; else a clear.
    put: bool,
}

/// Start-up, before the loop: the actions the last run left unfinished, then deleted, so the next
/// start does not name them again. `unclean` is the desktop's word on the last run; an unfinished
/// action makes it unclean too. A store that does not answer leaves the list empty.
pub(super) async fn at_start(store: &Store, unclean: bool, now: Timestamp) -> LastExitHost {
    let left = store.pending_actions().await.unwrap_or_else(|error| {
        tracing::warn!(error = %Cause(&error), "the unfinished file actions were not read");
        Vec::new()
    });
    for action in &left {
        if let Err(error) = store.clear_pending_action(action.matter).await {
            tracing::warn!(error = %Cause(&error), "an unfinished file action was not cleared");
        }
    }
    if unclean || !left.is_empty() {
        tracing::info!(
            target: LIFECYCLE,
            unclean,
            unfinished = left.len(),
            "the last run did not end cleanly"
        );
    }
    let interrupted: Vec<InterruptedAction> = left
        .into_iter()
        .map(|action| InterruptedAction {
            kind: action.kind,
            matter: action.matter,
            thread: action.thread,
            files: action.files,
        })
        .collect();
    LastExitHost {
        view: LastExit {
            unclean: unclean || !interrupted.is_empty(),
            run_started: now,
            interrupted,
        },
        recorded: BTreeSet::new(),
        waiting: BTreeMap::new(),
        clearing: BTreeSet::new(),
        held: BTreeMap::new(),
    }
}

/// `LastExit`: this run's answer.
pub(super) fn answer(actor: &Actor, reply: Reply<QueryResult>) {
    let _gone = reply.send(Ok(QueryResult::LastExit(actor.last_exit.view.clone())));
}

/// Puts `action` in the store, then runs `then` once it is durable. A put that fails is logged
/// and `then` runs all the same: the action goes ahead, and a crash during it would get the
/// notice's general line only. Counted as a matter job, so shutdown waits for it.
pub(super) fn record(actor: &mut Actor, action: PendingAction, then: Waiter) {
    let matter = action.matter;
    actor.last_exit.recorded.insert(matter);
    actor.last_exit.waiting.insert(matter, then);
    if actor.last_exit.clearing.contains(&matter) {
        // Put once the delete is back (`on_recorded`): the delete counts as the job till then.
        actor.last_exit.held.insert(matter, action);
        return;
    }
    put(actor, action);
}

/// Sends `action`'s put to the store, a matter job until it comes back.
fn put(actor: &mut Actor, action: PendingAction) {
    let matter = action.matter;
    actor.work.matter_jobs += 1;
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = store.put_pending_action(action).await;
        Input::Recorded(Box::new(Recorded {
            result,
            matter,
            put: true,
        }))
    });
}

/// A record's put came back.
pub(super) fn on_recorded(actor: &mut Actor, recorded: Recorded) {
    let Recorded {
        result,
        matter,
        put,
    } = recorded;
    actor.work.matter_jobs = actor.work.matter_jobs.saturating_sub(1);
    if let Err(error) = &result {
        tracing::warn!(%matter, put, error = %Cause(error), "a file action's record failed");
    }
    if put {
        if let Some(then) = actor.last_exit.waiting.remove(&matter) {
            then(actor, Ok(()));
        }
        return;
    }
    actor.last_exit.clearing.remove(&matter);
    if let Some(action) = actor.last_exit.held.remove(&matter) {
        self::put(actor, action);
    }
}

/// `matter`'s writer passed on: its recorded action, if any, is over. Nothing waits for the
/// delete but a later record for the matter (`record`).
pub(super) fn finished(actor: &mut Actor, matter: MatterId) {
    if !actor.last_exit.recorded.remove(&matter) {
        return;
    }
    actor.last_exit.clearing.insert(matter);
    actor.work.matter_jobs += 1;
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = store.clear_pending_action(matter).await;
        Input::Recorded(Box::new(Recorded {
            result,
            matter,
            put: false,
        }))
    });
}

/// The record of a file action on `matter`, asked from `thread`, at `at`.
pub(super) fn action(
    matter: MatterId,
    kind: clauseo_protocol::InterruptedKind,
    files: Option<u64>,
    thread: Option<ThreadId>,
    at: Timestamp,
) -> PendingAction {
    PendingAction {
        matter,
        kind,
        files: files.map(clauseo_protocol::Count::clamped),
        thread,
        at,
    }
}
