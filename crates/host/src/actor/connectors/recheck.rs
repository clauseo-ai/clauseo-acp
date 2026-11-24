//! Case law in the skills and tools list (`ListAddons`): its status there
//! (`CaseLaw::addon_status`), the fresh liveness check a read of the list starts while it is
//! `unreachable`, and `AddonsChanged` when that status moves.
//!
//! The list never waits on the network: a read answers with the state as it is and, while case
//! law is `unreachable` (the check failed, or none answered yet), connectors are on and wired,
//! starts one check in the background ([`Job::Recheck`]), never while one runs and never
//! within [`RECHECK_INTERVAL`] of the last one's start. Its result goes through the actor's
//! usual `publish`, which calls [`changed`].

use std::time::Duration;

use clauseo_protocol::AddonStatus;

use super::jobs::Job;
use crate::actor::Actor;

/// The least time between two checks a read of the list starts.
pub(crate) const RECHECK_INTERVAL: Duration = Duration::from_secs(30);

/// The re-check's pacing, and the status the list was last told of.
#[derive(Debug, Default)]
pub(crate) struct Recheck {
    /// When the last re-check began, on the host's monotonic clock.
    last: Option<Duration>,
    /// Whether one is in flight.
    running: bool,
    /// Case law's status when `AddonsChanged` last went out for it, or the list last read it.
    shown: Option<AddonStatus>,
}

/// Case law's status in the list now.
pub(in crate::actor) fn status(actor: &Actor) -> AddonStatus {
    actor.connectors.state.addon_status()
}

/// The list is being read: notes the status it shows and, while it is `unreachable`, starts a
/// re-check when one is due.
pub(in crate::actor) fn on_listed(actor: &mut Actor) {
    let status = status(actor);
    actor.connectors.recheck.shown = Some(status);
    if status != AddonStatus::Unreachable
        || !actor.effective.settings().tools.connectors
        || actor.connectors.wired.is_none()
    {
        return;
    }
    let now = actor.now().mono;
    let recheck = &mut actor.connectors.recheck;
    let recent = recheck
        .last
        .is_some_and(|last| now.saturating_sub(last) < RECHECK_INTERVAL);
    if recheck.running || recent {
        return;
    }
    recheck.running = true;
    recheck.last = Some(now);
    if let Err(error) = super::spawn(actor, None, Job::Recheck) {
        actor.connectors.recheck.running = false;
        tracing::warn!(error = %crate::log_file::Cause(&error), "case law was not checked again");
    }
}

/// A re-check ended, with a result or not.
pub(super) fn ended(actor: &mut Actor) {
    actor.connectors.recheck.running = false;
}

/// The connector state may have changed: `AddonsChanged` when case law's status differs from
/// the one last shown.
pub(super) fn changed(actor: &mut Actor) {
    let now = status(actor);
    if actor.connectors.recheck.shown.replace(now) != Some(now) {
        crate::actor::query::addons::changed(actor);
    }
}
