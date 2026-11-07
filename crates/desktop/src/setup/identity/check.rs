//! The identity check's steps on the backend: S18's sign-in states as they come
//! ([`on_state`]), one check of the login's account ([`check`]), the launch env's exemption
//! ([`exempt`], [`exemption_changed`]), and what a changed account does ([`account_changed`]).

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source, sign_in_limit};
use clauseo_host::StopFor;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use futures::future::{Either, select};

use super::refresh::{Ended, Plan, forced_share};
use super::watch::{stamp, stamps};
use super::{Judged, Stamp, Ticket, Verdict, Window};
use crate::bridge::lock;
use crate::setup::backend::AgentsBackend;
use crate::setup::compose::is_running;
use crate::setup::hook::{replaces_login, setup_vars_with};
use crate::setup::login_check::LOGIN_FILE;
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
        backend.compose.set_unconfirmed(agent, false);
        identity.refresh.signed_in(agent);
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
        match settle(backend, agent, &read, None, now, false) {
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
    let read = read_account(backend, agent, begun).await;
    let unwritten_again =
        matches!(read, Read::Unwritten(_)) && identity.refresh.unwritten_before(agent);
    if unwritten_again || *read.account() == AccountRead::Refused {
        // A second answer with no rewrite (the refresh fails with the network up), or Codex's
        // refused login: unknown, and it sticks until a forced read confirms or a sign-in.
        identity.refresh.stick(agent);
    }
    let now = backend.compose.monotonic();
    let confirming = matches!(read, Read::Rewritten(..));
    let verdict = settle(
        backend,
        agent,
        read.account(),
        Some(ticket),
        now,
        confirming,
    );
    tracing::info!(%agent, kind = verdict.kind(), "identity check");
    unconfirmed(backend, agent, &read, unwritten_again, verdict);
    let ended = match read {
        Read::Plain(_) => None,
        Read::Missed(_) => Some(Ended::Missed),
        Read::Unwritten(_) => Some(Ended::Unwritten),
        Read::Rewritten(_, after) if verdict == Verdict::Same => {
            tracing::info!(%agent, "identity: login confirmed by a forced refresh");
            Some(Ended::Confirmed(after))
        }
        Read::Rewritten(..) => Some(Ended::Unconfirmed),
    };
    if let Some(ended) = ended {
        identity.refresh.forced_ended(agent, ended, now);
    }
    if verdict == Verdict::Changed {
        account_changed(backend, agent).await;
    } else {
        refusal::sync(backend, agent).await;
    }
    verdict
}

/// How a check read the account (`refresh.rs`).
enum Read {
    /// Without a refresh.
    Plain(AccountRead),
    /// A forced read that confirmed nothing, and the read that decides this launch: the plain
    /// fallback after no answer, or a refused login.
    Missed(AccountRead),
    /// A forced read that answered with no rewrite, and its answer.
    Unwritten(AccountRead),
    /// A forced read that rewrote the login file to this stamp, and the plain read of it.
    Rewritten(AccountRead, Stamp),
}

impl Read {
    fn account(&self) -> &AccountRead {
        match self {
            Read::Plain(read)
            | Read::Missed(read)
            | Read::Unwritten(read)
            | Read::Rewritten(read, _) => read,
        }
    }
}

/// Refresh-on-check's unknown (Codex's refused login, or a second forced answer with no
/// rewrite) shows the sign-in-again card, its cause logged by kind; any verdict but unknown
/// takes it away (a held unknown stays unknown until a forced read confirms or a sign-in).
fn unconfirmed(
    backend: &AgentsBackend,
    agent: &AgentId,
    read: &Read,
    unwritten_again: bool,
    verdict: Verdict,
) {
    let cause = if verdict != Verdict::Unknown {
        None
    } else if *read.account() == AccountRead::Refused {
        Some("refresh_refused")
    } else if unwritten_again {
        Some("refresh_unwritten")
    } else {
        return;
    };
    if let Some(kind) = cause {
        tracing::warn!(%agent, kind, "identity: sign in again");
    }
    backend.compose.set_unconfirmed(agent, cause.is_some());
}

/// `agent`'s account, all its reads within one [`super::CHECK_BOUND`] (`Failed` past it): for
/// Codex with refresh-on-check on, a read with `refreshToken: true` when its plan says so (one
/// at a time in the process, let go at its share of the bound: its app-server's stdin closed, never
/// killed), then one
/// plain read of the file if Codex rewrote it, or the plain fallback after no answer.
async fn read_account(backend: &AgentsBackend, agent: &AgentId, now: Duration) -> Read {
    let identity = &backend.wiring.identity;
    let refresh = &identity.refresh;
    let budget = std::pin::pin!(AsyncIoTimer.after(identity.bound));
    let login = codex_login(backend, agent).filter(|_| refresh.on);
    let forced = login.filter(|login| refresh.plan(agent, stamp(login), now) == Plan::Forced);
    let Some(login) = forced else {
        let read = within(backend.agents.read_account(agent), budget).await;
        return Read::Plain(read.unwrap_or(AccountRead::Failed));
    };
    tracing::info!(%agent, "identity: forced refresh");
    let agents = Arc::clone(&backend.agents);
    let who = agent.clone();
    let flight = refresh.forced(agent, login, move || agents.read_account_refreshed(&who));
    let share = AsyncIoTimer.after(forced_share(identity.bound));
    let rewritten = match within(flight, share).await {
        Some(reply) if reply.read == AccountRead::Refused => {
            tracing::info!(%agent, "identity: forced refresh refused the login");
            return Read::Missed(AccountRead::Refused);
        }
        Some(reply) if reply.read != AccountRead::Failed && reply.rewrote() => {
            tracing::info!(%agent, "identity: forced refresh rewrote the login; reading it");
            Some(reply.after)
        }
        Some(reply) if reply.read != AccountRead::Failed => {
            tracing::info!(%agent, "identity: forced refresh rewrote nothing; not confirmed");
            return Read::Unwritten(reply.read);
        }
        answered => {
            if answered.is_none() {
                // Given up at its share: the next caller starts another, and once the last
                // one lets go the read is dropped: its app-server's stdin is closed.
                refresh.abandon(agent);
            }
            tracing::info!(%agent, "identity: forced refresh had no answer; unforced check used");
            None
        }
    };
    let read = within(backend.agents.read_account(agent), budget).await;
    let read = read.unwrap_or(AccountRead::Failed);
    match rewritten {
        Some(after) => Read::Rewritten(read, after),
        None => Read::Missed(read),
    }
}

/// `read`'s answer, or `None` once `bound` ends first.
async fn within<T>(read: impl Future<Output = T>, bound: impl Future<Output = ()>) -> Option<T> {
    let read = std::pin::pin!(read);
    let bound = std::pin::pin!(bound);
    match select(read, bound).await {
        Either::Left((read, _)) => Some(read),
        Either::Right(((), _)) => None,
    }
}

/// Codex's login file in `agent`'s home (for a reused login, the hard link of the lawyer's);
/// `None` for Claude or an unknown agent.
fn codex_login(backend: &AgentsBackend, agent: &AgentId) -> Option<PathBuf> {
    if AgentKind::of(agent) != Some(AgentKind::Codex) {
        return None;
    }
    let info = backend.known.get(agent)?;
    Some(info.home.join(LOGIN_FILE))
}

/// The verdict of `read` for `agent`, set unless `ticket`'s check was overtaken; the first
/// account seen with none recorded, before any launch, is recorded, holding for the stamps the
/// entry has (a check's, taken as it began). While refresh-on-check's unknown sticks
/// (`Refresh::stick`), only a `confirming` read (the plain read of a forced read's rewrite)
/// naming the recorded account lifts it, or records: any other stays unknown.
fn settle(
    backend: &AgentsBackend,
    agent: &AgentId,
    read: &AccountRead,
    ticket: Option<Ticket>,
    now: Duration,
    confirming: bool,
) -> Verdict {
    let identity = &backend.wiring.identity;
    // The login files after the read: a no-email sign-in's window records only while they are
    // as it left them, and any change breaks it.
    let after = stamps(backend, agent);
    identity.entry(agent, |entry| {
        let _broke = entry.break_window(&after, now);
    });
    let stuck = identity.refresh.stuck(agent);
    let judged = match identity.judge(agent, read, now, &after) {
        Judged::Record(_) if stuck && !confirming => Judged::Is(Verdict::Unknown),
        judged => judged,
    };
    let current = identity.entry(agent, |entry| {
        ticket.is_none_or(|Ticket(generation)| generation == entry.generation)
    });
    // On macOS Claude may keep its login in the Keychain, which no stamp sees: a no-email
    // sign-in's window lasts for its first check only, whatever it read or whatever overtook it
    // (a record this check makes shuts it after).
    let keychain = identity.keychain_fallback && AgentKind::of(agent) == Some(AgentKind::Claude);
    if keychain && ticket.is_some() {
        identity.entry(agent, |entry| {
            if matches!(entry.window, Window::Open(_)) {
                entry.window = Window::Broken;
            }
        });
    }
    if !current {
        return identity.verdict(agent);
    }
    if matches!(read, AccountRead::Account(_)) {
        identity.named(agent);
    }
    match judged {
        Judged::Record(hash) => {
            tracing::info!(%agent, kind = "first", "identity recorded");
            identity.record(agent, Some(hash), now, None)
        }
        Judged::Is(verdict) => identity.entry(agent, |entry| {
            // A changed account stays until a sign-in records again; a held unknown until a
            // forced read confirms or a sign-in.
            let confirms = confirming && verdict == Verdict::Same;
            let held = stuck && matches!(verdict, Verdict::Same | Verdict::Idle) && !confirms;
            if entry.verdict != Verdict::Changed {
                entry.verdict = if held { Verdict::Unknown } else { verdict };
            }
            // Only a status that failed, timed out or was refused is tried again on the timer;
            // a held unknown is tried again by its forced re-check.
            entry.again = !stuck && matches!(read, AccountRead::Failed | AccountRead::Refused);
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
