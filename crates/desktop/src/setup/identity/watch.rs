//! What the identity check watches, and the follower that watches it: each agent's login files
//! by metadata alone (a stat every [`POLL`]: file id and last write, never the contents), and
//! the checks launches ask for.
//!
//! - **Codex**: `auth.json` in Clauseo's agent home (its own login, or the hard link of a reused
//!   one), and the lawyer's `~/.codex/auth.json` while the login is reused.
//! - **Claude**: the store `CLAUDE_SECURESTORAGE_CONFIG_DIR` points a session at, when it is a
//!   file: `<home>/.credentials.json` for Clauseo's own login (the entry sets the variable to
//!   the home), the lawyer's `~/.claude/.credentials.json` for a reused one (the variable is
//!   empty: Claude's default store). On Linux and Windows Claude keeps its login in that file.
//!   On macOS it keeps it in the Keychain, and the file is only its fallback: with no file
//!   there, a check runs before a launch once the last one is [`KEYCHAIN_STALE`] old (the
//!   launch meets `agent.identity_checking` and asks for it through the host's launch retry).
//!
//! A change of any stamp (a file that appears, goes, is replaced or is written, a token refresh
//! included) checks at once, whether or not sessions run, signed in or not while an account is
//! recorded. With an account recorded and no stamps the verdict holds for, the look checks; it
//! never takes what is there as the baseline. An unknown verdict is checked again every
//! [`UNKNOWN_AGAIN`]; a macOS Claude login with an account recorded (its Keychain may hold the
//! login, which no stat sees) is due again every [`KEYCHAIN_STALE`], files or not. While the
//! launch env carries a key or a provider, nothing is looked at: a key login, not checked; once
//! it ends, the login is checked at once. A sign-in through Clauseo under way is not checked
//! until it ends or its bound passes.
//!
//! The follower runs each agent's check on its own (a status run that hangs to its bound holds
//! up no other agent), starts every agent's startup check at once, and fails closed when it
//! panics ([`fail_closed`], `setup/start.rs`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_agents::file_stamp;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use futures::channel::mpsc::UnboundedReceiver;
use futures::future::{BoxFuture, FutureExt as _};
use futures::stream::{FuturesUnordered, StreamExt as _};

use super::check::exempt;
use super::{Entry, KEYCHAIN_STALE, POLL, Stamp, UNKNOWN_AGAIN, Verdict, check};
use crate::bridge::lock;
use crate::setup::backend::AgentsBackend;
use crate::setup::login_check::{LAWYER_CODEX, LOGIN_FILE};
use crate::setup::refusal;

/// Claude's default store folder under the lawyer's home.
const LAWYER_CLAUDE: &str = ".claude";

/// The login files of `kind`'s login in `home` (Clauseo's agent home), `reused` or not.
pub(crate) fn watched(
    kind: AgentKind,
    home: &Path,
    lawyer_home: Option<&Path>,
    reused: bool,
) -> Vec<PathBuf> {
    let store = clauseo_sandbox::CLAUDE_STORE_FILES[0];
    match (kind, reused) {
        (AgentKind::Codex, false) => vec![home.join(LOGIN_FILE)],
        (AgentKind::Codex, true) => {
            let mut files = vec![home.join(LOGIN_FILE)];
            files.extend(lawyer_home.map(|lawyer| lawyer.join(LAWYER_CODEX).join(LOGIN_FILE)));
            files
        }
        (AgentKind::Claude, false) => vec![home.join(store)],
        (AgentKind::Claude, true) => lawyer_home
            .map(|lawyer| lawyer.join(LAWYER_CLAUDE).join(store))
            .into_iter()
            .collect(),
    }
}

/// `path`'s stat.
fn stamp(path: &Path) -> Stamp {
    match file_stamp(path) {
        Ok(Some(stamp)) => Stamp::File(stamp),
        Ok(None) => Stamp::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stamp::Missing,
        Err(error) => Stamp::Unreadable(error.kind()),
    }
}

/// The stamps of `agent`'s login files now.
pub(crate) fn stamps(backend: &AgentsBackend, agent: &AgentId) -> Vec<Stamp> {
    let (Some(kind), Some(info)) = (AgentKind::of(agent), backend.known.get(agent)) else {
        return Vec::new();
    };
    let lawyer = backend.wiring.identity.lawyer_home.as_deref();
    let reused = backend.wiring.reused.get(agent);
    watched(kind, &info.home, lawyer, reused)
        .iter()
        .map(|path| stamp(path))
        .collect()
}

/// What one look found for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Nothing,
    /// Check now.
    Check,
    /// A macOS Keychain login's last check is stale: due before the next launch.
    Stale,
    /// The launch env carries a key or a provider now: not checked.
    KeyLogin,
}

/// What a look at `entry` decides, given the login files' `stamps` now, whether the launch
/// env replaces the login (`exempt`), whether it is a macOS Claude login (`keychain`), whether
/// an account is `recorded`, and the time `now`. The state table in decision record 0048.
fn decide(
    entry: &mut Entry,
    stamps: Vec<Stamp>,
    exempt: bool,
    keychain: bool,
    recorded: bool,
    now: Duration,
) -> Look {
    if exempt {
        if matches!(entry.verdict, Verdict::KeyLogin | Verdict::Changed) {
            return Look::Nothing;
        }
        entry.verdict = Verdict::KeyLogin;
        entry.generation += 1;
        return Look::KeyLogin;
    }
    if entry.verdict == Verdict::KeyLogin {
        // The key or the provider is gone: the login is what launches use again.
        entry.verdict = Verdict::Due;
        entry.generation += 1;
        return Look::Check;
    }
    if entry.expecting(now) {
        return Look::Nothing;
    }
    // A sign-in's bound passed: checked as any other from here.
    entry.expect_end();
    if matches!(entry.verdict, Verdict::Checking | Verdict::Changed) {
        return Look::Nothing;
    }
    let Some(before) = &entry.stamps else {
        if recorded || entry.verdict == Verdict::Due {
            return Look::Check;
        }
        entry.stamps = Some(stamps);
        return Look::Nothing;
    };
    if *before != stamps {
        return Look::Check;
    }
    let age = entry.checked_at.map(|at| now.saturating_sub(at));
    let older = |bound: Duration| age.is_none_or(|age| age >= bound);
    if entry.verdict == Verdict::Unknown && older(UNKNOWN_AGAIN) {
        return Look::Check;
    }
    let timed = matches!(entry.verdict, Verdict::Idle | Verdict::Same);
    if keychain && recorded && timed && older(KEYCHAIN_STALE) {
        entry.verdict = Verdict::Due;
        return Look::Stale;
    }
    Look::Nothing
}

fn look(backend: &AgentsBackend, agent: &AgentId, now: Duration) -> Look {
    let identity = &backend.wiring.identity;
    let stamps = stamps(backend, agent);
    let keychain = identity.keychain_fallback && AgentKind::of(agent) == Some(AgentKind::Claude);
    let exempt = exempt(backend, agent);
    let recorded = identity.recorded_now(agent);
    let (seen, bound_passed) = identity.entry(agent, |entry| {
        let pending = entry.expect.is_some();
        let seen = decide(entry, stamps, exempt, keychain, recorded, now);
        (seen, pending && entry.expect.is_none())
    });
    if bound_passed {
        tracing::info!(%agent, kind = "sign_in_bound", "identity: checked again");
    }
    seen
}

/// One look at every agent at `now` (the clock's monotonic time): the refusals the host could
/// not be told tried again (`setup/refusal.rs`), each look's refusal set, and the agents due a
/// check, which the caller runs.
async fn looks(backend: &AgentsBackend, now: Duration) -> Vec<AgentId> {
    let unsynced: Vec<AgentId> = lock(&backend.wiring.unsynced).iter().cloned().collect();
    for agent in &unsynced {
        refusal::sync(backend, agent).await;
    }
    let mut checks = Vec::new();
    for agent in backend.known.keys() {
        match look(backend, agent, now) {
            Look::Nothing => {}
            Look::Check => {
                tracing::info!(%agent, kind = "login_changed", "identity");
                checks.push(agent.clone());
            }
            Look::Stale => {
                tracing::info!(%agent, kind = "keychain_stale", "identity");
                refusal::sync(backend, agent).await;
            }
            Look::KeyLogin => {
                tracing::info!(%agent, kind = "key_login", "identity: not checked");
                refusal::sync(backend, agent).await;
            }
        }
    }
    checks
}

/// One look at every agent's login files at `now`, the checks it finds due run side by side
/// and waited for: what the follower does every [`POLL`], without its other work (the tests').
#[cfg(test)]
pub(crate) async fn poll_once(backend: &AgentsBackend, now: Duration) {
    let checks = looks(backend, now).await;
    futures::future::join_all(checks.iter().map(|agent| check(backend, agent))).await;
}

/// `agent`'s check, as one of the follower's running ones.
fn checking(backend: &AgentsBackend, agent: AgentId) -> BoxFuture<'_, ()> {
    async move {
        check(backend, &agent).await;
    }
    .boxed()
}

/// The checks a launch asks for (`Identities::ask`) as they come, run when the agent's check
/// is due; and a look every [`POLL`] between, the first at once (every agent's startup check).
/// Each check runs on its own beside the others. Ends when the asks' channel closes.
pub(crate) async fn follow_identity(
    backend: &AgentsBackend,
    asks: &mut UnboundedReceiver<AgentId>,
) {
    let mut running = FuturesUnordered::new();
    for agent in looks(backend, backend.compose.monotonic()).await {
        running.push(checking(backend, agent));
    }
    let mut tick = Box::pin(AsyncIoTimer.after(POLL)).fuse();
    loop {
        futures::select! {
            ask = asks.next() => {
                let Some(agent) = ask else { return };
                let verdict = backend.wiring.identity.verdict(&agent);
                if matches!(verdict, Verdict::Due | Verdict::KeyLogin) {
                    tracing::info!(%agent, kind = "launch_asked", "identity");
                    running.push(checking(backend, agent));
                }
            }
            () = tick => {
                tick = Box::pin(AsyncIoTimer.after(POLL)).fuse();
                for agent in looks(backend, backend.compose.monotonic()).await {
                    running.push(checking(backend, agent));
                }
            }
            () = running.select_next_some() => {}
        }
    }
}

/// The follower panicked: every agent not a key login nor already refused for a changed account
/// is made unknown, so `agent.identity_unknown` refuses its launches (fail closed) until a
/// check answers again (at once, once a follower runs: the last check's time is dropped).
pub(crate) async fn fail_closed(backend: &AgentsBackend) {
    for agent in backend.known.keys() {
        backend.wiring.identity.entry(agent, |entry| {
            if !matches!(entry.verdict, Verdict::KeyLogin | Verdict::Changed) {
                entry.verdict = Verdict::Unknown;
                entry.checked_at = None;
                entry.generation += 1;
            }
        });
        refusal::sync(backend, agent).await;
    }
}
