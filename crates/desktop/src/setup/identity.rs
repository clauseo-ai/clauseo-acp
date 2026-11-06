//! R17's identity check of the shared login (decision record 0048, "identity check (R17)").
//! Each matter's agents use the one login (Codex's `auth.json` by hard link, Claude's store
//! through `CLAUDE_SECURESTORAGE_CONFIG_DIR`), which stays writable for token refresh, so an
//! agent could swap another account into it. Desktop records which account each agent's login
//! belongs to and refuses or stops the agent's launches once it is not that account.
//!
//! - **The identity** comes only from the agents' own status output: S18's `SignedIn { email }`
//!   and `Agents::read_account` (`claude auth status --json`; `codex login status`, then
//!   `account/read`). Never a token, never a credential file's contents. It is kept and written
//!   only as an [`AccountHash`] (SHA-256 of the trimmed, lower-cased email), in
//!   `<data_dir>/agents/identity.json` (`identity/store.rs`), so a swap while Clauseo is closed
//!   shows at the next start.
//! - **Recorded** at each sign-in or reuse made through Clauseo ([`Identities::expect`] marks
//!   one under way; its `SignedIn` records first, before anything compares), and at the first
//!   `SignedIn` when nothing is recorded (first run).
//! - **A `SignedIn` that is not one of those** (S18's status check, at start or when a window
//!   shows Plans) is compared: another account is [`Verdict::Changed`]. Before R17 the pump
//!   took such a state as a new sign-in and restarted the agents on it.
//! - **The verdict is cached** per agent with the [`FileStamp`]s (file id and last write) of
//!   its login files, and checked again at once when one changes (`identity/watch.rs`, a stat
//!   every [`POLL`], sessions running or not), or, for a Claude login kept only in the macOS
//!   Keychain (no file), before a launch once the last check is [`KEYCHAIN_STALE`] old.
//! - **The refusals** (`setup/refusal.rs`): [`Verdict::refusal`]. `agent.identity_checking`
//!   while a check runs or is due (a launch waits for it, and asks for it through the host's
//!   launch retry); `agent.identity_unknown` when status failed, timed out or named no account;
//!   `signin.account_changed` once it named another account, the agents stopped, until a new
//!   sign-in records again (`identity/check.rs`).
//!
//! Logs name the agent and a fixed kind, never an email, a hash or a path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead, FileStamp, SigninState};
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::Files;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use self::store::Store;
use crate::bridge::lock;

pub(crate) use self::check::{Seen, check, on_state};
pub(crate) use self::watch::follow_identity;
#[cfg(test)]
pub(crate) use self::watch::poll_once;

mod check;
mod store;
mod watch;

/// How often the login files are looked at (metadata only).
pub(crate) const POLL: Duration = Duration::from_secs(2);
/// How old the last check of a Keychain-only Claude login may be before a launch checks again.
pub(crate) const KEYCHAIN_STALE: Duration = Duration::from_mins(10);
/// How long after an unknown verdict the check runs again by itself.
pub(crate) const UNKNOWN_AGAIN: Duration = Duration::from_secs(30);
/// The most one check may take, past the status runs' own limits (5 s each, and 10 s for
/// Codex's app-server): then it is unknown.
pub(crate) const CHECK_BOUND: Duration = Duration::from_secs(20);

/// What desktop knows of an agent's login's account now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Verdict {
    /// Nothing to check: not signed in with a plan login.
    #[default]
    Idle,
    /// A check is due: at start, with an account recorded, until the first status answers; or
    /// a Keychain-only login's last check is stale. The next launch asks for it.
    Due,
    /// A check runs.
    Checking,
    /// The status named the recorded account.
    Same,
    /// The status failed, timed out or named no account; or the record cannot be read.
    Unknown,
    /// The status named another account: the agents were stopped.
    Changed,
}

impl Verdict {
    /// The launch refusal this verdict puts on the agent, if any.
    pub(crate) fn refusal(self) -> Option<ErrorCode> {
        match self {
            Verdict::Idle | Verdict::Same => None,
            Verdict::Due | Verdict::Checking => Some(ErrorCode::AgentIdentityChecking),
            Verdict::Unknown => Some(ErrorCode::AgentIdentityUnknown),
            Verdict::Changed => Some(ErrorCode::SigninAccountChanged),
        }
    }

    /// The log's fixed word for it.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Verdict::Idle => "idle",
            Verdict::Due => "due",
            Verdict::Checking => "checking",
            Verdict::Same => "same",
            Verdict::Unknown => "unknown",
            Verdict::Changed => "changed",
        }
    }
}

/// What one login file's stat found: its stamp, none there, or the stat's error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stamp {
    /// The file, by id and last write.
    File(FileStamp),
    /// No file (or a link, which is not the login).
    Missing,
    /// The stat failed for another reason.
    Unreadable(std::io::ErrorKind),
}

/// One agent's state.
#[derive(Debug, Default)]
struct Entry {
    verdict: Verdict,
    /// A sign-in or reuse made through Clauseo is under way: its `SignedIn` records.
    expect: bool,
    /// The login files' stamps the verdict holds for; `None` until the next look takes them.
    stamps: Option<Vec<Stamp>>,
    /// When the last check or record was, on the clock's monotonic time.
    checked_at: Option<Duration>,
    /// Bumped at each record and each sign-out, so a check that started before is dropped.
    generation: u64,
}

/// The state behind the lock.
#[derive(Debug, Default)]
struct Inner {
    agents: BTreeMap<AgentId, Entry>,
    /// Each agent's recorded account.
    recorded: BTreeMap<AgentId, AccountHash>,
    /// `identity.json` could not be read at start: no record can be trusted until a sign-in
    /// through Clauseo writes one.
    unreadable: BTreeSet<AgentId>,
}

/// A check that began: its agent's generation then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket(u64);

/// Every agent's identity state, the record on disk, and the checks asked for by launches.
pub(crate) struct Identities {
    store: Store,
    /// The lawyer's home, whose `.codex` and `.claude` hold a reused login.
    lawyer_home: Option<PathBuf>,
    /// Whether a Claude login with no file is checked before a launch once stale (macOS).
    keychain_fallback: bool,
    /// The bound on one check ([`CHECK_BOUND`]; shorter in tests).
    pub(crate) bound: Duration,
    inner: Mutex<Inner>,
    asks: UnboundedSender<AgentId>,
    asked: Mutex<Option<UnboundedReceiver<AgentId>>>,
}

impl Identities {
    /// No record on disk and no lawyer's home: the tests' and a start with no data folder's.
    pub(crate) fn in_memory() -> Self {
        Self::with_store(Store::none(), None, std::iter::empty())
    }

    /// Reads `<data_dir>/agents/identity.json` through `files`: each agent of `known` with an
    /// account recorded starts [`Verdict::Due`]; when the file cannot be read, every one does,
    /// and none counts as recorded (fail closed). The error's kind alone is logged.
    pub(crate) fn load<'a>(
        data_dir: &std::path::Path,
        files: Arc<dyn Files + Send + Sync>,
        lawyer_home: Option<PathBuf>,
        known: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        Self::with_store(Store::at(data_dir, files), lawyer_home, known)
    }

    fn with_store<'a>(
        store: Store,
        lawyer_home: Option<PathBuf>,
        known: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        let mut inner = Inner::default();
        let read = store.load();
        for agent in known {
            let due = if let Ok(recorded) = &read {
                recorded.contains_key(agent)
            } else {
                inner.unreadable.insert(agent.clone());
                true
            };
            let verdict = if due { Verdict::Due } else { Verdict::Idle };
            let entry = Entry {
                verdict,
                ..Entry::default()
            };
            inner.agents.insert(agent.clone(), entry);
        }
        match read {
            Ok(recorded) => inner.recorded = recorded,
            Err(kind) => tracing::warn!(?kind, "the identity record could not be read"),
        }
        let (asks, asked) = unbounded();
        Self {
            store,
            lawyer_home,
            keychain_fallback: cfg!(target_os = "macos"),
            bound: CHECK_BOUND,
            inner: Mutex::new(inner),
            asks,
            asked: Mutex::new(Some(asked)),
        }
    }

    /// Tests: the Keychain fallback on or off, whatever the OS.
    #[cfg(test)]
    pub(crate) fn with_keychain_fallback(mut self, on: bool) -> Self {
        self.keychain_fallback = on;
        self
    }

    /// `agent`'s verdict now.
    pub(crate) fn verdict(&self, agent: &AgentId) -> Verdict {
        lock(&self.inner)
            .agents
            .get(agent)
            .map(|entry| entry.verdict)
            .unwrap_or_default()
    }

    /// The launch refusal `agent`'s verdict puts on it, if any.
    pub(crate) fn refusal(&self, agent: &AgentId) -> Option<ErrorCode> {
        self.verdict(agent).refusal()
    }

    /// A sign-in or reuse through Clauseo starts for `agent`: its `SignedIn` records.
    pub(crate) fn expect(&self, agent: &AgentId) {
        self.entry(agent, |entry| entry.expect = true);
    }

    /// The call that would have signed `agent` in was refused: nothing is under way.
    pub(crate) fn expect_none(&self, agent: &AgentId) {
        self.entry(agent, |entry| entry.expect = false);
    }

    /// A launch met `agent.identity_checking`: the follower runs the check when it is due.
    pub(crate) fn ask(&self, agent: &AgentId) {
        // Closed only once the follower is gone.
        let _gone = self.asks.unbounded_send(agent.clone());
    }

    /// The asks, once, for the follower.
    pub(crate) fn take_asks(&self) -> Option<UnboundedReceiver<AgentId>> {
        lock(&self.asked).take()
    }

    fn entry<T>(&self, agent: &AgentId, change: impl FnOnce(&mut Entry) -> T) -> T {
        change(lock(&self.inner).agents.entry(agent.clone()).or_default())
    }

    /// Records `hash` as `agent`'s account, and writes the record when it changed (a write
    /// that fails is logged by its kind; the record holds in memory). The agent's verdict is
    /// `Same`, or `Unknown` with no account, from `now`.
    fn record(&self, agent: &AgentId, hash: Option<AccountHash>, now: Duration) -> Verdict {
        let mut inner = lock(&self.inner);
        inner.unreadable.remove(agent);
        let verdict = match hash {
            Some(hash) => {
                inner.recorded.insert(agent.clone(), hash);
                Verdict::Same
            }
            None => Verdict::Unknown,
        };
        let entry = inner.agents.entry(agent.clone()).or_default();
        entry.verdict = verdict;
        entry.expect = false;
        entry.stamps = None;
        entry.checked_at = Some(now);
        entry.generation += 1;
        let recorded = inner.recorded.clone();
        drop(inner);
        if let Err(error) = self.store.save(&recorded) {
            tracing::warn!(%agent, kind = ?error.kind(), "the identity record was not written");
        }
        verdict
    }

    /// The verdict of `read` against `agent`'s record.
    fn judge(&self, agent: &AgentId, read: &AccountRead) -> Judged {
        let inner = lock(&self.inner);
        if inner.unreadable.contains(agent) {
            return Judged::Is(Verdict::Unknown);
        }
        match (read, inner.recorded.get(agent)) {
            (AccountRead::Account(hash), Some(recorded)) if hash == recorded => {
                Judged::Is(Verdict::Same)
            }
            (AccountRead::Account(_), Some(_)) => Judged::Is(Verdict::Changed),
            (AccountRead::Account(hash), None) => Judged::Record(hash.clone()),
            _ => Judged::Is(Verdict::Unknown),
        }
    }

    /// Starts a check of `agent` with the login files at `stamps`: `None` (no check) when it
    /// has nothing to check, one runs, or a sign-in through Clauseo is under way.
    fn begin(&self, agent: &AgentId, stamps: Vec<Stamp>) -> Option<Ticket> {
        self.entry(agent, |entry| {
            let checkable = matches!(
                entry.verdict,
                Verdict::Due | Verdict::Same | Verdict::Unknown
            );
            if !checkable || entry.expect {
                return None;
            }
            entry.verdict = Verdict::Checking;
            entry.stamps = Some(stamps);
            Some(Ticket(entry.generation))
        })
    }

    /// Whether an account is recorded for `agent` now.
    fn recorded_now(&self, agent: &AgentId) -> bool {
        lock(&self.inner).recorded.contains_key(agent)
    }

    /// `agent`'s recorded account.
    #[cfg(test)]
    pub(crate) fn recorded(&self, agent: &AgentId) -> Option<AccountHash> {
        lock(&self.inner).recorded.get(agent).cloned()
    }
}

/// What a status's account means against the record.
enum Judged {
    /// This verdict.
    Is(Verdict),
    /// No account recorded (first run): record this one.
    Record(AccountHash),
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

#[cfg(test)]
mod tests;
