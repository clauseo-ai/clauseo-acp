//! The identity check's steps on the backend: S18's sign-in states as they come
//! ([`on_state`]), one check of the login's account ([`check`]), and what a changed account
//! does ([`account_changed`]).

use clauseo_agents::{AccountRead, SigninState, Source};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::AgentId;
use futures::future::{Either, select};

use super::{Judged, Ticket, Verdict, signed_in_account};
use crate::setup::backend::AgentsBackend;
use crate::setup::compose::is_running;
use crate::setup::refusal;

/// What [`on_state`] made of a state, for the pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    /// Nothing for the identity: a running state, or a resting one.
    Other,
    /// A `SignedIn` of a sign-in or reuse through Clauseo, which recorded its account. The
    /// pump restarts the agents on it even when S18's last state named the same account (a
    /// sign-in again after a change), so the questions held since run.
    SignedIn,
    /// A `SignedIn` that recorded the first account seen (nothing was recorded).
    Recorded,
    /// A `SignedIn` of the recorded account, or one whose account cannot be told.
    Checked,
    /// A `SignedIn` of another account than the recorded one: the agents were stopped, and
    /// the pump must not take it as a new sign-in.
    Changed,
}

/// S18's new `state` for `agent`, before the pump handles it. A `SignedIn` of a sign-in or
/// reuse through Clauseo records its account first, so nothing compares it with the account it
/// replaced; another `SignedIn` is compared with the record ([`Seen::Changed`] runs
/// [`account_changed`]). A resting state that is not `SignedIn` ends any sign-in under way and
/// leaves nothing to check, but a changed account stays until a sign-in records again.
pub(crate) async fn on_state(
    backend: &AgentsBackend,
    agent: &AgentId,
    state: &SigninState,
) -> Seen {
    let identity = &backend.wiring.identity;
    let now = backend.compose.monotonic();
    if is_running(state) {
        return Seen::Other;
    }
    let Some(read) = signed_in_account(state) else {
        let failed = matches!(state, SigninState::Failed { .. });
        identity.entry(agent, |entry| {
            entry.expect = false;
            if !failed && entry.verdict != Verdict::Changed {
                entry.verdict = Verdict::Idle;
                entry.generation += 1;
            }
        });
        refusal::sync(backend, agent).await;
        return Seen::Other;
    };
    let expected = identity.entry(agent, |entry| entry.expect);
    let seen = if expected {
        let hash = match read {
            AccountRead::Account(hash) => Some(hash),
            _ => None,
        };
        identity.record(agent, hash, now);
        tracing::info!(%agent, kind = "recorded", "identity");
        backend.compose.set_account_changed(agent, false);
        Seen::SignedIn
    } else {
        let had = identity.recorded_now(agent);
        match settle(backend, agent, &read, None, now) {
            Verdict::Changed => Seen::Changed,
            _ if had => Seen::Checked,
            _ => Seen::Recorded,
        }
    };
    if seen == Seen::Changed {
        account_changed(backend, agent).await;
    } else {
        refusal::sync(backend, agent).await;
    }
    seen
}

/// One check of `agent`'s account: `agent.identity_checking` while S18's read runs (bounded by
/// [`super::CHECK_BOUND`]), then the verdict: the refusal lifted when it is the recorded
/// account, `agent.identity_unknown` when the read failed, timed out or named none,
/// [`account_changed`] when it named another. Nothing when the agent has nothing to check, a
/// check runs, or a sign-in is under way; a check that a record or a sign-out overtook leaves
/// their verdict.
pub(crate) async fn check(backend: &AgentsBackend, agent: &AgentId) -> Verdict {
    let identity = &backend.wiring.identity;
    let stamps = super::watch::stamps(backend, agent);
    let Some(ticket) = identity.begin(agent, stamps) else {
        return identity.verdict(agent);
    };
    refusal::sync(backend, agent).await;
    let bound = std::pin::pin!(AsyncIoTimer.after(identity.bound));
    let read = select(backend.agents.read_account(agent), bound).await;
    let read = match read {
        Either::Left((read, _)) => read,
        Either::Right(((), _)) => AccountRead::Failed,
    };
    let now = backend.compose.monotonic();
    let verdict = settle(backend, agent, &read, Some(ticket), now);
    tracing::info!(%agent, kind = verdict.kind(), "identity check");
    if verdict == Verdict::Changed {
        account_changed(backend, agent).await;
    } else {
        refusal::sync(backend, agent).await;
    }
    verdict
}

/// The verdict of `read` for `agent`, set unless `ticket`'s check was overtaken; the first
/// account seen with none recorded is recorded.
fn settle(
    backend: &AgentsBackend,
    agent: &AgentId,
    read: &AccountRead,
    ticket: Option<Ticket>,
    now: std::time::Duration,
) -> Verdict {
    let identity = &backend.wiring.identity;
    let judged = identity.judge(agent, read);
    let current = identity.entry(agent, |entry| {
        ticket.is_none_or(|Ticket(generation)| generation == entry.generation)
    });
    if !current {
        return identity.verdict(agent);
    }
    match judged {
        Judged::Record(hash) => {
            tracing::info!(%agent, kind = "first", "identity recorded");
            identity.record(agent, Some(hash), now)
        }
        Judged::Is(verdict) => identity.entry(agent, |entry| {
            // A changed account stays until a sign-in records again.
            if entry.verdict != Verdict::Changed {
                entry.verdict = verdict;
            }
            entry.checked_at = Some(now);
            entry.verdict
        }),
    }
}

/// `agent`'s account is not the recorded one: `signin.account_changed` on its launches first,
/// so none starts meanwhile; then its agents stopped ([`stop_for_account_change`]); then the
/// card says sign in again (`SignIn::Failed { code: signin.account_changed }`). A reused login
/// also gets S18's status check, which ends the reuse as `ReusedChanged` (its automatic leave).
pub(crate) async fn account_changed(backend: &AgentsBackend, agent: &AgentId) {
    tracing::warn!(%agent, kind = "account_changed", "identity: stopping the agents");
    backend
        .wiring
        .identity
        .entry(agent, |entry| entry.verdict = Verdict::Changed);
    refusal::sync(backend, agent).await;
    stop_for_account_change(backend, agent).await;
    backend.compose.set_account_changed(agent, true);
    let reused = matches!(
        backend.agents.signin_state(agent),
        Some(SigninState::SignedIn {
            source: Source::Reused,
            ..
        })
    );
    if reused && let Err(error) = backend.agents.check_status(agent) {
        tracing::warn!(%agent, %error, "no status check after an account change");
    }
}

/// Stops `agent`'s agents because its login's account changed. The one place R17 stops them:
/// the host's `stop_agents_for(agent, StopFor::SignInAgain)`, which keeps the stopped turns
/// held for the new sign-in's `restart_agents`, replaces `stop_agents` here once it lands.
pub(crate) async fn stop_for_account_change(backend: &AgentsBackend, agent: &AgentId) {
    if let Err(error) = backend.host.stop_agents(agent.clone()).await {
        tracing::warn!(%agent, code = error.code().as_str(), "identity: agents not stopped");
    }
}
