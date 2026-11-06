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
//! included) checks at once, whether or not sessions run. An unknown verdict is checked again
//! every [`UNKNOWN_AGAIN`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use clauseo_agents::file_stamp;
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use futures::StreamExt as _;
use futures::future::{Either, select};

use super::{KEYCHAIN_STALE, POLL, Stamp, UNKNOWN_AGAIN, Verdict, check};
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
    /// A Keychain-only login's last check is stale: due before the next launch.
    Stale,
}

/// One look at every agent's login files at `now` (the clock's monotonic time): a check for
/// each whose stamps changed, or whose unknown verdict is due again; `agent.identity_checking`
/// for a Keychain-only Claude login whose last check is stale. The first look after a record
/// takes the stamps the verdict holds for.
pub(crate) async fn poll_once(backend: &AgentsBackend, now: Duration) {
    for agent in backend.known.keys() {
        match look(backend, agent, now) {
            Look::Nothing => {}
            Look::Check => {
                tracing::info!(%agent, kind = "login_changed", "identity");
                check(backend, agent).await;
            }
            Look::Stale => {
                tracing::info!(%agent, kind = "keychain_stale", "identity");
                refusal::sync(backend, agent).await;
            }
        }
    }
}

fn look(backend: &AgentsBackend, agent: &AgentId, now: Duration) -> Look {
    let identity = &backend.wiring.identity;
    let stamps = stamps(backend, agent);
    let keychain = identity.keychain_fallback
        && AgentKind::of(agent) == Some(AgentKind::Claude)
        && stamps.iter().all(|stamp| *stamp == Stamp::Missing);
    identity.entry(agent, |entry| {
        if entry.expect || !matches!(entry.verdict, Verdict::Same | Verdict::Unknown) {
            return Look::Nothing;
        }
        let Some(before) = &entry.stamps else {
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
        if keychain && entry.verdict == Verdict::Same && older(KEYCHAIN_STALE) {
            entry.verdict = Verdict::Due;
            return Look::Stale;
        }
        Look::Nothing
    })
}

/// The checks a launch asks for (`Identities::ask`) as they come, run when the agent's check
/// is due; and a look every [`POLL`] between. Ends when the asks' channel closes; nothing
/// when another follower took it.
pub(crate) async fn follow_identity(backend: &AgentsBackend) {
    let Some(mut asks) = backend.wiring.identity.take_asks() else {
        return;
    };
    loop {
        let tick = std::pin::pin!(AsyncIoTimer.after(POLL));
        match select(asks.next(), tick).await {
            Either::Left((Some(agent), _)) => {
                if backend.wiring.identity.verdict(&agent) == Verdict::Due {
                    tracing::info!(%agent, kind = "launch_asked", "identity");
                    check(backend, &agent).await;
                }
            }
            Either::Left((None, _)) => return,
            Either::Right(((), _)) => poll_once(backend, backend.compose.monotonic()).await,
        }
    }
}
