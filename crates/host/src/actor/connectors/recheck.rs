//! Case law in the skills and tools list (`ListAddons`): its status there
//! (`CaseLaw::addon_status`), the checks a read of the list starts in the background, and
//! `AddonsChanged` when that status moves. Case law is left out until its credential has been
//! read once ([`status`]).
//!
//! The list never waits on the network: a read answers with the state as it is and, with
//! connectors on and wired, may start one check ([`Job::Recheck`]):
//! - not signed in and not known to be offered (no check yet, no answer, or a preview price
//!   list): a fresh liveness check ([`Kind::Liveness`]), at most every [`LIVENESS_INTERVAL`];
//! - signed in: the credential and the account ([`Kind::Account`], holding the one-a-minute
//!   read slot), at most every [`ACCOUNT_INTERVAL`], so a key revoked on the server stops reading
//!   `ready` before a paid call fails.
//!
//! Each kind runs one at a time; one started more than [`STUCK_AFTER`] ago counts as ended, so
//! a lost result never stops the checks for good. Results go through the actor's usual
//! `publish`, which calls [`changed`].

use std::time::Duration;

use clauseo_connectors::CredentialStatus;
use clauseo_protocol::AddonStatus;

use super::jobs::Job;
use crate::actor::Actor;

/// The least time between two liveness checks a read of the list starts.
pub(crate) const LIVENESS_INTERVAL: Duration = Duration::from_secs(30);

/// The least time between two credential and account checks a read of the list starts.
pub(crate) const ACCOUNT_INTERVAL: Duration = Duration::from_mins(5);

/// A check started this long ago counts as ended, even with no result.
pub(crate) const STUCK_AFTER: Duration = Duration::from_mins(2);

/// What a read of the list checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A fresh `pricing.json` (a cached answer that is not live is fetched again).
    Liveness,
    /// The credential and the account, holding the read slot.
    Account,
}

/// One kind's pacing: when the last began, on the host's monotonic clock, and whether it runs.
#[derive(Debug, Default)]
struct Pace {
    last: Option<Duration>,
    running: bool,
}

impl Pace {
    /// Whether one may start at `now`: none running (or the one running is stuck) and the last
    /// began at least `interval` ago.
    fn due(&self, now: Duration, interval: Duration) -> bool {
        let since = |last: Duration| now.saturating_sub(last);
        let stuck = self.last.is_none_or(|last| since(last) > STUCK_AFTER);
        let spaced = self.last.is_none_or(|last| since(last) >= interval);
        (!self.running || stuck) && spaced
    }
}

/// The checks' pacing, and the status the list was last told of.
#[derive(Debug, Default)]
pub(crate) struct Recheck {
    liveness: Pace,
    account: Pace,
    /// Case law's status (`None`: left out) when `AddonsChanged` last went out for it, or the
    /// list last read it.
    shown: Option<AddonStatus>,
    /// Whether `shown` has been set at all.
    seen: bool,
}

impl Recheck {
    fn pace(&mut self, kind: Kind) -> &mut Pace {
        match kind {
            Kind::Liveness => &mut self.liveness,
            Kind::Account => &mut self.account,
        }
    }
}

/// Case law's status in the list now: `not_offered` on an install with no data connectors;
/// `None`, left out of the list, until the credential has been read once (start-up, or a
/// keychain prompt still open), so a signed-in lawyer never sees it flash `needs_setup`.
pub(in crate::actor) fn status(actor: &Actor) -> Option<AddonStatus> {
    if actor.connectors.wired.is_none() {
        return Some(AddonStatus::NotOffered);
    }
    let state = &actor.connectors.state;
    state.credential.is_some().then(|| state.addon_status())
}

/// The list is being read: notes the status it shows and starts the check that is due, if any.
pub(in crate::actor) fn on_listed(actor: &mut Actor) {
    actor.connectors.recheck.shown = status(actor);
    actor.connectors.recheck.seen = true;
    if !actor.effective.settings().tools.connectors || actor.connectors.wired.is_none() {
        return;
    }
    let state = &actor.connectors.state;
    let kind = match state.credential {
        // Not read yet: the read that lists it starts the check.
        None => return,
        _ if state.signing_in => return,
        Some(CredentialStatus::SignedIn(_)) => Kind::Account,
        Some(CredentialStatus::SignedOut | CredentialStatus::KeyRevoked) => return,
        Some(CredentialStatus::NotAdded) if state.live() => return,
        Some(CredentialStatus::NotAdded) => Kind::Liveness,
    };
    let now = actor.now().mono;
    let interval = match kind {
        Kind::Liveness => LIVENESS_INTERVAL,
        Kind::Account => ACCOUNT_INTERVAL,
    };
    if !actor.connectors.recheck.pace(kind).due(now, interval) {
        return;
    }
    // The account check holds the read slot: none while a read runs or was just made.
    if kind == Kind::Account && !actor.connectors.reads.take(now) {
        return;
    }
    let pace = actor.connectors.recheck.pace(kind);
    pace.running = true;
    pace.last = Some(now);
    if let Err(error) = super::spawn(actor, None, Job::Recheck(kind)) {
        actor.connectors.recheck.pace(kind).running = false;
        tracing::warn!(error = %crate::log_file::Cause(&error), "case law was not checked again");
    }
}

/// A check of `kind` ended, with a result or not.
pub(super) fn ended(actor: &mut Actor, kind: Kind) {
    actor.connectors.recheck.pace(kind).running = false;
}

/// The connector state may have changed: `AddonsChanged` when case law's status differs from
/// the one last shown, or case law joins or leaves the list. Before the first (none shown), no
/// app holds a list to read again.
pub(super) fn changed(actor: &mut Actor) {
    let now = status(actor);
    let recheck = &mut actor.connectors.recheck;
    let was = std::mem::replace(&mut recheck.shown, now);
    if std::mem::replace(&mut recheck.seen, true) && was != now {
        crate::actor::query::addons::changed(actor);
    }
}
