//! The case-law hint on the actor (R24, PR6). When a turn ends whose web searches looked like
//! case law (`connectors/hint.rs`, flagged as each record is logged), the host checks, in order:
//! `tools.connectors` on; connectors wired; the connector known to be `NotAdded`; the matter
//! not marked in `hints.json`. Only then does it ask liveness (a `Check` job with no account
//! read, so a lawyer who never searches case law sends nothing to Clauseo's server); when it
//! comes back live, the same checks run again, and it publishes
//! `CaseLawHint { matter, thread, turn }` to every app subscriber and marks the matter `shown`.
//! The event is an offer, not part of the record: it is never logged.
//!
//! `DismissCaseLawHint` marks the matter `dismissed`. Either mark keeps the hint away for good.
//! A matter waits for liveness at most once at a time; its first turn waiting is the one named.
//! It stops waiting on every way the check ends: liveness known, the job not started, or the
//! job ending without a result (`HintState::no_liveness`).

use std::collections::BTreeMap;

use clauseo_connectors::CredentialStatus;
use clauseo_protocol::{HostEvent, MatterId, ThreadId, TurnId};

use super::super::Actor;
use super::{Job, spawn};
use crate::connectors::hint::file::{Hints, Mark};
use crate::error::Error;

/// The hint's state: the marks, read at the first need, and the matters waiting on liveness.
#[derive(Debug, Default)]
pub(crate) struct HintState {
    file: Option<Hints>,
    waiting: BTreeMap<MatterId, (ThreadId, TurnId)>,
}

impl HintState {
    /// A `Check` job ended without liveness (a panic: its drop guard, `jobs.rs`'s
    /// `Unfinished`, posts `Change::NoLiveness`): the matters waiting stop waiting, so a later
    /// case-law turn asks again.
    pub(super) fn no_liveness(&mut self) {
        self.waiting.clear();
    }

    /// In tests: `matter` waits on liveness for `thread`'s `turn`.
    #[cfg(test)]
    pub(super) fn wait(&mut self, matter: MatterId, thread: ThreadId, turn: TurnId) {
        self.waiting.insert(matter, (thread, turn));
    }

    /// In tests: whether any matter waits on liveness.
    #[cfg(test)]
    pub(super) fn waits(&self) -> bool {
        !self.waiting.is_empty()
    }
}

/// A turn of `thread` in `matter` ended, and its web searches looked like case law.
pub(crate) fn turn_ended(actor: &mut Actor, matter: MatterId, thread: ThreadId, turn: TurnId) {
    if !allowed(actor, matter) || actor.connectors.hint.waiting.contains_key(&matter) {
        return;
    }
    actor.connectors.hint.waiting.insert(matter, (thread, turn));
    if let Err(error) = spawn(actor, None, Job::Check { slot: false }) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "no liveness for the case-law hint");
        actor.connectors.hint.waiting.remove(&matter);
    }
}

/// Liveness came back: each matter waiting gets its hint if the connector is live and the
/// checks still pass.
pub(super) fn liveness_known(actor: &mut Actor) {
    if actor.connectors.hint.waiting.is_empty() {
        return;
    }
    let live = actor
        .connectors
        .state
        .liveness
        .as_ref()
        .is_some_and(|liveness| liveness.live);
    let waiting = std::mem::take(&mut actor.connectors.hint.waiting);
    if !live {
        return;
    }
    for (matter, (thread, turn)) in waiting {
        if !allowed(actor, matter) {
            continue;
        }
        if let Err(error) = hints(actor).mark(matter, Mark::Shown) {
            tracing::warn!(error = %crate::log_file::Cause(&error), "hints.json was not written");
        }
        super::super::app::publish(
            actor,
            &HostEvent::CaseLawHint {
                matter,
                thread,
                turn,
            },
        );
    }
}

/// `DismissCaseLawHint` for `matter`: marked `dismissed`, in memory even if the file write
/// fails (logged).
pub(super) fn dismiss(actor: &mut Actor, matter: MatterId) -> Result<(), Error> {
    actor.check_matter(matter)?;
    actor.connectors.hint.waiting.remove(&matter);
    if let Err(error) = hints(actor).mark(matter, Mark::Dismissed) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "hints.json was not written");
    }
    Ok(())
}

/// The checks before liveness: the switch, the wiring, the connector not added, no mark.
fn allowed(actor: &mut Actor, matter: MatterId) -> bool {
    actor.effective.settings().tools.connectors
        && actor.connectors.wired.is_some()
        && matches!(
            actor.connectors.state.credential,
            Some(CredentialStatus::NotAdded)
        )
        && !hints(actor).marked(matter)
}

fn hints(actor: &mut Actor) -> &mut Hints {
    let data_dir = actor.effective.data_dir().to_path_buf();
    actor
        .connectors
        .hint
        .file
        .get_or_insert_with(|| Hints::new(&data_dir))
}
