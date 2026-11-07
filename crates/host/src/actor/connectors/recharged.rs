//! A relay's paid `recharge` result on the actor: the balance it reports, the account read after
//! it (one at a time, repeats paced, one more when a newer result came during it), and the
//! balance epoch, so a job older than a resumed charge never shows a balance from before it.

use std::sync::atomic::Ordering;

use clauseo_protocol::RechargeOutcome;

use super::jobs::{Change, Finished, Job};
use super::limits::ReadFor;
use super::spawn;
use crate::actor::Actor;
use crate::connectors::ResumedBalance;

/// A relay's `recharge` tool result said paid (the relay dropped the connectors crate's account
/// copy): out of credit ends and a balance shows at once. The watch on that recharge, if any,
/// sets the outcome at its next poll.
///
/// - `Known`: the balance after the call it resumed (`resumed.balance_credits`, or the one its
///   last charge line names) shows, and nothing is read. The top-level `balance_credits` is from
///   before that call's charge, and a read made at once may be refused by the connector or
///   answered from before the charge is booked. The relay bumped the balance epoch before
///   posting it: an account read begun before, and a recharge answer from a job begun before,
///   show no balance.
/// - `Unknown`: a call was resumed but its balance after does not read, so the top-level one
///   (stale) does not show; the epoch was bumped (as above) and the account is read, as below,
///   every time (a resumed call is new spend, never a repeat).
/// - `NoCall`: `balance` shows, and the account is read now, not at the one-a-minute pace, for
///   what the call it resumed spent.
///
/// Reads after paid results go one at a time; a result that wants one while one is in flight
/// gets one more after it. A repeat of the result the last read was for (an agent calling
/// `recharge` again, [`ReadFor::repeats`]) waits for the one-a-minute slot instead.
pub(super) fn recharged(
    actor: &mut Actor,
    recharge: Option<String>,
    (balance, resumed): (Option<i64>, ResumedBalance),
) {
    let now = actor.now();
    let connectors = &mut actor.connectors;
    let state = &mut connectors.state;
    state.out_of_credit = None;
    let wants = match resumed {
        ResumedBalance::Known(credits) => {
            state.set_balance(credits, now.wall);
            let gate = &mut connectors.recharged;
            gate.last = None;
            gate.again = None;
            return;
        }
        ResumedBalance::Unknown => ReadFor {
            recharge,
            resumed: true,
        },
        ResumedBalance::NoCall => {
            if let Some(credits) = balance {
                state.set_balance(credits, now.wall);
            }
            ReadFor {
                recharge,
                resumed: false,
            }
        }
    };
    if !state.signed_in() {
        return;
    }
    let gate = &mut connectors.recharged;
    if gate.last.as_ref().is_some_and(|last| wants.repeats(last)) {
        if !gate.in_flight
            && connectors.reads.take(now.mono)
            && let Err(error) = spawn(actor, None, Job::Read)
        {
            tracing::warn!(error = %crate::log_file::Cause(&error), "no balance read");
        }
        return;
    }
    if gate.in_flight {
        gate.again = Some(wants);
        return;
    }
    read(actor, wants);
}

/// Starts the read after the paid result `wants`.
fn read(actor: &mut Actor, wants: ReadFor) {
    let gate = &mut actor.connectors.recharged;
    gate.in_flight = true;
    gate.last = Some(wants);
    if let Err(error) = spawn(actor, None, Job::Recharged) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "no balance read");
    }
}

/// The read after a paid result ended (a panic too): the one a newer result asked for while it
/// was in flight starts, while signed in.
pub(super) fn read_ended(actor: &mut Actor) {
    let gate = &mut actor.connectors.recharged;
    gate.in_flight = false;
    let Some(wants) = gate.again.take() else {
        return;
    };
    if actor.connectors.state.signed_in() {
        read(actor, wants);
    }
}

/// Drops what `finished` read of the balance from before the latest resumed charge (the balance
/// epoch moved since): its account and the out of credit its read answered when the epoch moved
/// after the read began, and the balance its recharge's answer carried when it moved after the
/// job began (the answer came from a poll at some point in between). The outcome still applies.
///
/// Returns the recharge to read for when a paid recharge's balance went this way (its answer's,
/// or its read's) with no account left to show (its read refused, or dropped): the paid balance
/// then shows from [`catch_up`].
pub(super) fn drop_stale_balance(actor: &Actor, finished: &mut Finished) -> Option<String> {
    let epoch = actor.connectors.recharged.epoch.load(Ordering::SeqCst);
    let read_stale = finished.read_at.is_some_and(|at| at != epoch);
    if read_stale {
        finished.account = None;
        finished.out_of_credit = None;
    }
    let Change::Recharge {
        recharge,
        outcome,
        balance,
    } = &mut finished.change
    else {
        return None;
    };
    let answer_stale = finished.started != epoch && balance.take().is_some();
    let paid = *outcome == Some(RechargeOutcome::Paid) && finished.account.is_none();
    (paid && (read_stale || answer_stale)).then(|| recharge.clone())
}

/// One account read for a paid `recharge` whose balance [`drop_stale_balance`] dropped: started
/// now, or after the read in flight, while signed in.
pub(super) fn catch_up(actor: &mut Actor, recharge: String) {
    if !actor.connectors.state.signed_in() {
        return;
    }
    let wants = ReadFor {
        recharge: Some(recharge),
        resumed: false,
    };
    if actor.connectors.recharged.in_flight {
        actor.connectors.recharged.again = Some(wants);
    } else {
        read(actor, wants);
    }
}
