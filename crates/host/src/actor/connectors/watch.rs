//! The recharge watch on the actor: one watch at a time, numbered, on the recharge of an
//! out-of-credit answer or of an `AddCredits` once its pay link arrives (an `AddCredits` still
//! opening its recharge leaves the current watch in place, so a refused one loses nothing).

use clauseo_connectors::{CancelFlag, PayUrl};
use clauseo_protocol::{CommandId, ErrorCode};

use super::jobs::{Job, Refusal};
use super::{answer, publish, spawn};
use crate::actor::Actor;
use crate::connectors::state::OutOfCredit;

/// The refusal of an `AddCredits` whose recharge opened after a newer one, a sign-out, a
/// remove or the switch going off.
const RECHARGE_ENDED: &str = "the recharge was replaced or ended before its pay link";

/// One recharge watch.
pub(super) struct Watch {
    pub(super) number: u64,
    pub(super) recharge: Option<String>,
    pub(super) cancel: CancelFlag,
}

impl Watch {
    pub(super) fn follows(&self, recharge: &str) -> bool {
        self.recharge.as_deref() == Some(recharge)
    }
}

/// Cancels the recharge watch, if any, and starts following `recharge` with `cancel`; returns
/// the new watch's number. The pay link held goes unless the watch already follows
/// `recharge`: it is an `AddCredits` link (`on_pay_link`), for the old watch's recharge, which
/// `OpenPayLink` must not open once nothing watches it. The last recharge's outcome goes with
/// it: a new recharge is watched.
pub(super) fn replace_watch(
    actor: &mut Actor,
    recharge: Option<String>,
    cancel: CancelFlag,
) -> u64 {
    let same = recharge.as_deref().is_some_and(|recharge| {
        actor
            .connectors
            .watch
            .as_ref()
            .is_some_and(|watch| watch.follows(recharge))
    });
    if !same {
        actor.connectors.state.pay_url = None;
        actor.connectors.state.last_recharge = None;
    }
    end_watch(actor);
    actor.connectors.watches += 1;
    let number = actor.connectors.watches;
    actor.connectors.watch = Some(Watch {
        number,
        recharge,
        cancel,
    });
    number
}

/// A new `AddCredits`: an older one still opening its recharge is cancelled; returns the
/// number its watch takes once its pay link arrives.
pub(super) fn open_recharge(actor: &mut Actor, cancel: CancelFlag) -> u64 {
    if let Some((_, older)) = actor.connectors.opening.take() {
        older.cancel();
    }
    actor.connectors.watches += 1;
    let number = actor.connectors.watches;
    actor.connectors.opening = Some((number, cancel));
    number
}

/// Cancels the recharge watch and an `AddCredits` still opening its recharge.
pub(super) fn end_recharges(actor: &mut Actor) {
    if let Some((_, opening)) = actor.connectors.opening.take() {
        opening.cancel();
    }
    end_watch(actor);
}

/// Cancels the recharge watch, if any.
pub(super) fn end_watch(actor: &mut Actor) {
    if let Some(watch) = actor.connectors.watch.take() {
        watch.cancel.cancel();
    }
}

/// `AddCredits`' pay link: the newest `AddCredits`' recharge takes over the watch and the
/// command is answered; a stale one is refused.
pub(super) fn on_pay_link(
    actor: &mut Actor,
    command: Option<CommandId>,
    watch: u64,
    (recharge, pay_url): (String, PayUrl),
) {
    let reply = match actor.connectors.opening.take() {
        // The newest `AddCredits`: its recharge takes over the watch.
        Some((number, cancel)) if number == watch => {
            end_watch(actor);
            actor.connectors.watch = Some(Watch {
                number,
                recharge: Some(recharge),
                cancel,
            });
            actor.connectors.state.pay_url = Some(pay_url);
            // A new recharge is watched: the last one's outcome is over.
            actor.connectors.state.last_recharge = None;
            Ok(())
        }
        // A newer `AddCredits`, a sign-out, a remove or the switch going off came
        // first, and cancelled this one's watch.
        other => {
            actor.connectors.opening = other;
            Err(Refusal::new(ErrorCode::CommandConflict, RECHARGE_ENDED))
        }
    };
    answer(actor, command, reply);
}

/// A watch's thread ended: an `AddCredits` refused before its pay link is no longer opening,
/// and the current watch, if it was this one, goes with its pay link.
pub(super) fn on_watch_ended(actor: &mut Actor, watch: u64) {
    // An `AddCredits` refused before its pay link: the current watch stays.
    if actor
        .connectors
        .opening
        .as_ref()
        .is_some_and(|(number, _)| *number == watch)
    {
        actor.connectors.opening = None;
    }
    if actor
        .connectors
        .watch
        .as_ref()
        .is_some_and(|current| current.number == watch)
    {
        actor.connectors.watch = None;
        actor.connectors.state.pay_url = None;
        publish(actor);
    }
}

/// Out of credit, with the payment the connector opened: its pay link when it is ours, and a
/// watch on its recharge unless that one is watched already.
pub(super) fn out_of_credit(actor: &mut Actor, payment: clauseo_connectors::PaymentRef) {
    actor.connectors.state.out_of_credit = Some(OutOfCredit {
        pay_url: payment.url,
    });
    let Some(recharge) = payment.recharge_id else {
        return;
    };
    let watched = actor
        .connectors
        .watch
        .as_ref()
        .is_some_and(|watch| watch.follows(&recharge));
    if watched {
        return;
    }
    let cancel = CancelFlag::new();
    let number = replace_watch(actor, Some(recharge.clone()), cancel.clone());
    let job = Job::Watch {
        recharge,
        number,
        cancel,
    };
    if let Err(error) = spawn(actor, None, job) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "no watch on the recharge");
        actor.connectors.watch = None;
    }
}
