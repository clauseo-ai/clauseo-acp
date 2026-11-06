//! The identity check's steps on the backend: S18's sign-in states as they come
//! ([`on_state`]), one check of the login's account ([`check`]), the launch env's exemption
//! ([`exempt`], [`exemption_changed`]), and what a changed account does ([`account_changed`]).

use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source, sign_in_limit};
use clauseo_host::StopFor;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use futures::future::{Either, select};

use super::watch::stamps;
use super::{Judged, Ticket, Verdict, Window};
use crate::bridge::lock;
use crate::setup::backend::AgentsBackend;
use crate::setup::compose::is_running;
use crate::setup::hook::{replaces_login, setup_vars_with};
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

/// How long a sign-in through Clauseo for `agent` holds off the check at most, from its start
/// or its last running state: its run's own bound (`clauseo_agents::sign_in_limit`), else ten
/// minutes.
fn pending_bound(agent: &AgentId) -> Duration {
    AgentKind::of(agent).map_or(Duration::from_mins(10), sign_in_limit)
}

/// A sign-in or reuse through Clauseo starts for `agent`: its `SignedIn` records, holding for
/// the login files' stamps now (before its confirming status read), for its run's bound.
pub(crate) fn expect(backend: &AgentsBackend, agent: &AgentId) {
    let now = backend.compose.monotonic();
    let stamps = stamps(backend, agent);
    let identity = &backend.wiring.identity;
    identity.expect(agent, now, pending_bound(agent), stamps);
}

/// S18's new `state` for `agent`, before the pump handles it.
///
/// - A running state moves a pending sign-in's bound on, with the stamps before its next step.
/// - A `SignedIn` of a sign-in or reuse through Clauseo records its account first, holding for
///   the stamps from before its status read, so nothing compares it with the account it
///   replaced (with no email, the record goes and a check that may name an id records it).
/// - Another `SignedIn` is compared with the record ([`Seen::Changed`] runs
///   [`account_changed`]), unless a key login makes it moot, or it names no email, when a check
///   is asked for.
/// - A resting state that is not `SignedIn` ends any sign-in under way and makes the verdict
///   idle, or signed out after a sign-out through Clauseo (nothing refused), but the agent stays
///   watched while an account is recorded, its stamps kept; a changed account stays until a
///   sign-in records again, a key login holds, and a sign-in whose files moved while it was
///   pending stays due and is checked.
pub(crate) async fn on_state(
    backend: &AgentsBackend,
    agent: &AgentId,
    state: &SigninState,
) -> Seen {
    let identity = &backend.wiring.identity;
    let now = backend.compose.monotonic();
    if is_running(state) {
        if identity.entry(agent, |entry| entry.expect.is_some()) {
            let stamps = stamps(backend, agent);
            identity.expect_running(agent, now, pending_bound(agent), stamps);
        }
        return Seen::Other;
    }
    let Some(read) = signed_in_account(state) else {
        let failed = matches!(state, SigninState::Failed { .. });
        if !failed {
            identity.nameless(agent, false);
        }
        let signed_out = identity.is_signed_out(agent);
        let moved = identity.entry(agent, |entry| {
            // A sign-in that ends with no record after its files moved stays due, and checked.
            let moved = entry.expect_end();
            let kept = matches!(entry.verdict, Verdict::Changed | Verdict::KeyLogin);
            if !failed && !kept && !moved {
                entry.verdict = if signed_out {
                    Verdict::SignedOut
                } else {
                    Verdict::Idle
                };
                entry.generation += 1;
            }
            moved
        });
        if moved {
            identity.ask(agent);
        }
        refusal::sync(backend, agent).await;
        return Seen::Other;
    };
    let expected = identity.entry(agent, |entry| entry.expecting(now));
    let seen = if expected {
        if let AccountRead::Account(hash) = read {
            let before = identity.pending_stamps(agent);
            identity.record(agent, Some(hash), now, before);
            tracing::info!(%agent, kind = "recorded", "identity");
        } else {
            // No email: the status may name the account by its id; the check records it.
            let left = stamps(backend, agent);
            identity.forget(agent, now, pending_bound(agent), left);
            identity.ask(agent);
            tracing::info!(%agent, kind = "no_email", "identity");
        }
        backend.compose.set_account_changed(agent, false);
        Seen::SignedIn
    } else if exempt(backend, agent) {
        if identity.key_login(agent) {
            tracing::info!(%agent, kind = "key_login", "identity: not checked");
        }
        Seen::Checked
    } else if read == AccountRead::NoAccount {
        // A state with no email cannot tell; the check's status may name an id.
        identity.nameless(agent, true);
        identity.entry(agent, |entry| {
            if entry.verdict != Verdict::Changed {
                entry.verdict = Verdict::Due;
            }
        });
        identity.ask(agent);
        Seen::Checked
    } else {
        let had = identity.recorded_now(agent);
        match settle(backend, agent, &read, None, now) {
            Verdict::Changed => Seen::Changed,
            _ if had => Seen::Checked,
            _ if identity.recorded_now(agent) => Seen::Recorded,
            _ => Seen::Checked,
        }
    };
    if seen == Seen::Changed {
        account_changed(backend, agent).await;
    } else {
        refusal::sync(backend, agent).await;
    }
    seen
}

/// Whether the identity check does not apply to `agent` (a key login): it is firm-managed
/// (`SettingsRead::managed`: a policy locks `agents.<id>`, or the e2e build's
/// `E2E_AGENTS_MANAGED`), with no Clauseo install for a status read to run (a documented gap:
/// it still launches on Clauseo's login home); or its launch env, as the launch hook builds it
/// (`setup/hook.rs`'s `setup_vars`), carries a credential that replaces the shared login: the
/// API key variable the hook sets (a stored key), or a Claude provider switch (Bedrock, Vertex,
/// Foundry). An endpoint alone does not count, nor does the lawyer's own `agents.<id>`.
pub(crate) fn exempt(backend: &AgentsBackend, agent: &AgentId) -> bool {
    let Some(kind) = AgentKind::of(agent) else {
        return false;
    };
    if lock(&backend.wiring.applied).managed.contains(agent) {
        return true;
    }
    let vars = setup_vars_with(Some(&backend.keys), &backend.wiring.plans, agent, kind);
    replaces_login(kind, &vars)
}

/// A key, an endpoint or a provider of `agent` changed: the exemption is worked out again
/// from the launch env. One that ends makes the agent due a check (asked for at once) and puts
/// `agent.identity_checking` on its launches before this returns, so a relaunch after it waits
/// for the check; one that starts makes it a key login. Either drops a check in flight.
pub(crate) async fn exemption_changed(backend: &AgentsBackend, agent: &AgentId) {
    let identity = &backend.wiring.identity;
    if exempt(backend, agent) {
        if identity.key_login(agent) {
            tracing::info!(%agent, kind = "key_login", "identity: not checked");
        }
    } else if identity.key_login_ended(agent) {
        tracing::info!(%agent, kind = "key_login_ended", "identity");
        identity.ask(agent);
    }
    refusal::sync(backend, agent).await;
}

/// One check of `agent`'s account (none while it is [`exempt`]: a key login, not checked):
/// `agent.identity_checking` while S18's read runs (bounded by [`super::CHECK_BOUND`]), then the
/// verdict: the refusal lifted when it is the recorded account, `agent.identity_unknown` when
/// the read failed, timed out or named none, [`account_changed`] when it named another. The
/// login files' stamps are taken before the read, so a change during it shows at the next look.
/// Nothing when a check runs, the account changed, or a sign-in is under way; a check that a
/// record, a sign-out or a key login overtook leaves their verdict.
pub(crate) async fn check(backend: &AgentsBackend, agent: &AgentId) -> Verdict {
    let identity = &backend.wiring.identity;
    if exempt(backend, agent) {
        if identity.key_login(agent) {
            tracing::info!(%agent, kind = "key_login", "identity: not checked");
            refusal::sync(backend, agent).await;
        }
        return identity.verdict(agent);
    }
    let stamps = stamps(backend, agent);
    let begun = backend.compose.monotonic();
    let Some(ticket) = identity.begin(agent, stamps, begun) else {
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
/// account seen with none recorded, before any launch, is recorded, holding for the stamps the
/// entry has (a check's, taken as it began).
fn settle(
    backend: &AgentsBackend,
    agent: &AgentId,
    read: &AccountRead,
    ticket: Option<Ticket>,
    now: Duration,
) -> Verdict {
    let identity = &backend.wiring.identity;
    // The login files after the read: a no-email sign-in's window records only while they are
    // as it left them, and any change breaks it.
    let after = stamps(backend, agent);
    identity.entry(agent, |entry| {
        let _broke = entry.break_window(&after, now);
    });
    let judged = identity.judge(agent, read, now, &after);
    let current = identity.entry(agent, |entry| {
        ticket.is_none_or(|Ticket(generation)| generation == entry.generation)
    });
    if !current {
        return identity.verdict(agent);
    }
    if matches!(read, AccountRead::Account(_)) {
        identity.named(agent);
    }
    let verdict = match judged {
        Judged::Record(hash) => {
            tracing::info!(%agent, kind = "first", "identity recorded");
            identity.record(agent, Some(hash), now, None)
        }
        Judged::Is(verdict) => identity.entry(agent, |entry| {
            // A changed account stays until a sign-in records again.
            if entry.verdict != Verdict::Changed {
                entry.verdict = verdict;
            }
            // Only a status that failed or timed out is tried again on the timer.
            entry.again = matches!(read, AccountRead::Failed);
            entry.checked_at = Some(now);
            entry.verdict
        }),
    };
    // On macOS Claude may keep its login in the Keychain, which no stamp sees: a no-email
    // sign-in's window lasts for its first check only, whatever it read (a record shut it).
    let keychain = identity.keychain_fallback && AgentKind::of(agent) == Some(AgentKind::Claude);
    if keychain && ticket.is_some() {
        identity.entry(agent, |entry| {
            if matches!(entry.window, Window::Open(_)) {
                entry.window = Window::Broken;
            }
        });
    }
    verdict
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

/// Stops `agent`'s agents because its login's account changed, the one place R17 stops them:
/// the host's `stop_agents_for(agent, StopFor::SignInAgain)`. The host's hold stays until the
/// new sign-in's `restart_agents` (after it records the account again), so a prompt to the
/// agent meanwhile, in any thread, queues held as `sign_in` rather than failing, and runs then.
pub(crate) async fn stop_for_account_change(backend: &AgentsBackend, agent: &AgentId) {
    let stopped = backend
        .host
        .stop_agents_for(agent.clone(), StopFor::SignInAgain);
    if let Err(error) = stopped.await {
        tracing::warn!(%agent, code = error.code().as_str(), "identity: agents not stopped");
    }
}

/// What a `SignedIn` `state` says of the account: the account its email names, or none for
/// one with no email; `None` for any other state.
fn signed_in_account(state: &SigninState) -> Option<AccountRead> {
    let SigninState::SignedIn { email, .. } = state else {
        return None;
    };
    let hash = email.as_deref().and_then(AccountHash::of);
    Some(hash.map_or(AccountRead::NoAccount, AccountRead::Account))
}
