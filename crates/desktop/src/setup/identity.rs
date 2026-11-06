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
//!   one under way, for the sign-in run's own bound at most; its `SignedIn` records first,
//!   before anything compares; a change of its files meanwhile refuses until it lands), and,
//!   when nothing is recorded (first run, trusted on first use), from a status read before any
//!   launch of the agent passed the launch gate; after one, an account with no record is
//!   unknown.
//! - **A `SignedIn` that is not one of those** (S18's status check, at start or when a window
//!   shows Plans) is compared: another account is [`Verdict::Changed`]. Before R17 the pump
//!   took such a state as a new sign-in and restarted the agents on it.
//! - **The verdict is cached** per agent with the [`FileStamp`]s (file id, last write, last
//!   status change and size) of its login files, taken before the status read it holds for
//!   (never cleared: with an account recorded, no stamps means check), and checked again at
//!   once when one changes: at each launch (`identity/gate.rs`, the host's launch gate) and
//!   between (`identity/watch.rs`, a stat every [`POLL`], sessions running or not), each
//!   agent's check on its own. A macOS Claude login (the Keychain) is checked again before a
//!   launch once the last check is [`KEYCHAIN_STALE`] old.
//! - **The refusals** (`setup/refusal.rs`): [`Verdict::refusal`]. `agent.identity_checking`
//!   while a check runs or is due (a launch waits for it, and asks for it through the host's
//!   launch retry); `agent.identity_unknown` when status failed, timed out or named no account;
//!   `signin.account_changed` once it named another account, the agents stopped, until a new
//!   sign-in records again (`identity/check.rs`).
//!
//! - **No email:** the account id names the account, when the status gives one; neither is
//!   unknown. **A key login** (the launch env carries a stored API key or a Claude provider
//!   switch, never an endpoint alone) is not checked: [`Verdict::KeyLogin`].
//!
//! The state table in decision record 0048 ("Identity check (R17)") says, for each verdict,
//! whether it is watched, checked again on a timer, and refuses; [`Verdict`]'s docs follow it.
//!
//! Logs name the agent and a fixed kind, never an email, a hash or a path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::{AccountHash, AccountRead, FileStamp, SigninState};
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::Files;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use self::store::Store;
use crate::bridge::lock;

pub(crate) use self::check::{Seen, check, expect, on_state};
pub(crate) use self::check::{exempt, exemption_changed};
#[cfg(test)]
pub(crate) use self::gate::gate;
pub(crate) use self::gate::{GateSlot, launch_gate};
pub(crate) use self::verdict::Verdict;
pub(crate) use self::watch::{fail_closed, follow_identity};
#[cfg(test)]
pub(crate) use self::watch::{poll_once, stamps};

mod check;
mod gate;
mod store;
mod verdict;
mod watch;

/// How often the login files are looked at (metadata only).
pub(crate) const POLL: Duration = Duration::from_secs(2);
/// How old the last check of a Keychain-only Claude login may be before a launch checks again.
pub(crate) const KEYCHAIN_STALE: Duration = Duration::from_mins(10);
/// How long after an unknown verdict from a status that failed or timed out the check runs
/// again by itself.
pub(crate) const UNKNOWN_AGAIN: Duration = Duration::from_secs(30);
/// The most one check may take, past the status runs' own limits (5 s each, and 10 s for
/// Codex's app-server): then it is unknown.
pub(crate) const CHECK_BOUND: Duration = Duration::from_secs(20);

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
    /// A sign-in or reuse made through Clauseo is under way until this time on the clock's
    /// monotonic time (its run's bound, moved on at each running state): its `SignedIn`
    /// records. Past it, the agent is checked as any other.
    expect: Option<Duration>,
    /// The login files' stamps from before the pending sign-in's last step, so the confirming
    /// status read comes after them: what its record holds for.
    pending: Option<Vec<Stamp>>,
    /// The login files' stamps the verdict holds for, taken before the status read that gave
    /// it. `None` only when none was taken: with an account recorded, the next look checks.
    stamps: Option<Vec<Stamp>>,
    /// When the last check or record was, on the clock's monotonic time.
    checked_at: Option<Duration>,
    /// Bumped at each record, sign-out, and key login's start or end, so a check that started
    /// before is dropped.
    generation: u64,
    /// A launch of the agent passed the launch gate: from then on an account with no record is
    /// never trusted on first use.
    launched: bool,
    /// The login files moved from `pending` while the sign-in was pending: the verdict is due
    /// (`agent.identity_checking`) until the record lands; past the bound, unknown and checked.
    held: bool,
    /// An unknown verdict is checked again every [`UNKNOWN_AGAIN`]: it came from a status that
    /// failed or timed out, or from a panic (`fail_closed`).
    again: bool,
}

impl Entry {
    /// Whether a pending sign-in through Clauseo holds at `now`.
    fn expecting(&self, now: Duration) -> bool {
        self.expect.is_some_and(|until| now < until)
    }

    /// A pending sign-in is no longer under way: whether its files had moved (`held`).
    fn expect_end(&mut self) -> bool {
        self.expect = None;
        self.pending = None;
        std::mem::take(&mut self.held)
    }

    /// At a look or a launch while a sign-in is pending, the login files at `stamps`: when
    /// they differ from those the sign-in took, a verdict that refuses nothing becomes due,
    /// held until the record lands (R17 review M1). Whether it did now.
    fn hold_if_moved(&mut self, stamps: &[Stamp]) -> bool {
        let rests = matches!(
            self.verdict,
            Verdict::Idle | Verdict::Same | Verdict::SignedOut
        );
        let moved = self
            .pending
            .as_deref()
            .is_none_or(|before| before != stamps);
        if rests && moved {
            self.verdict = Verdict::Due;
            self.held = true;
            self.generation += 1;
        }
        rests && moved
    }
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
    /// S18 last said these are signed in with no email: a status naming no account then is
    /// unknown, not "nothing signed in", even with nothing recorded.
    nameless: BTreeSet<AgentId>,
    /// Signed out through Clauseo since the last record: a status naming no account is
    /// [`Verdict::SignedOut`], not unknown.
    signed_out: BTreeSet<AgentId>,
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
    /// The followers panicked past their restarts (`setup/supervise.rs`): every agent on the
    /// shared login is refused `agent.identity_unknown` (a changed one keeps its refusal),
    /// whatever its verdict, until Clauseo starts again.
    dead: AtomicBool,
}

impl Identities {
    /// No record on disk and no lawyer's home: the tests' and a start with no data folder's.
    pub(crate) fn in_memory() -> Self {
        Self::with_store(Store::none(), None, std::iter::empty())
    }

    /// Reads `<data_dir>/agents/identity.json` through `files`. Every agent of `known` starts
    /// [`Verdict::Due`], its startup check run as soon as the follower starts: with an account
    /// recorded, so a swap while Clauseo was closed shows; with none (first run), so the account
    /// trusted on first use is the one before any launch. When the file cannot be read, none
    /// counts as recorded (fail closed). The error's kind alone is logged.
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
            if read.is_err() {
                inner.unreadable.insert(agent.clone());
            }
            let entry = Entry {
                verdict: Verdict::Due,
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
            dead: AtomicBool::new(false),
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

    /// The launch refusal `agent`'s verdict puts on it, if any; `agent.identity_unknown` for
    /// any but a changed account once the followers are dead.
    pub(crate) fn refusal(&self, agent: &AgentId) -> Option<ErrorCode> {
        let verdict = self.verdict(agent);
        if self.dead.load(Ordering::SeqCst) && verdict != Verdict::Changed {
            return Some(ErrorCode::AgentIdentityUnknown);
        }
        verdict.refusal()
    }

    /// The followers will not run again (`setup/supervise.rs`): every refusal is pinned, as
    /// `dead` says.
    pub(crate) fn die(&self) {
        self.dead.store(true, Ordering::SeqCst);
    }

    /// A sign-out through Clauseo of `agent` starts (`true`), or was refused (`false`).
    pub(crate) fn signing_out(&self, agent: &AgentId, on: bool) {
        let mut inner = lock(&self.inner);
        if on {
            inner.signed_out.insert(agent.clone());
        } else {
            inner.signed_out.remove(agent);
        }
    }

    /// Whether `agent` was signed out through Clauseo since its last record.
    pub(super) fn is_signed_out(&self, agent: &AgentId) -> bool {
        lock(&self.inner).signed_out.contains(agent)
    }

    /// A status named an account for `agent`: a sign-out through Clauseo is over.
    pub(super) fn named(&self, agent: &AgentId) {
        lock(&self.inner).signed_out.remove(agent);
    }

    /// S18 says `agent` is signed in with no email (`true`), or no longer signed in.
    pub(super) fn nameless(&self, agent: &AgentId, nameless: bool) {
        let mut inner = lock(&self.inner);
        if nameless {
            inner.nameless.insert(agent.clone());
        } else {
            inner.nameless.remove(agent);
        }
    }

    /// Whether an account is recorded for `agent` now, or the record cannot be read (which is
    /// watched as one).
    pub(super) fn recorded_now(&self, agent: &AgentId) -> bool {
        let inner = lock(&self.inner);
        inner.recorded.contains_key(agent) || inner.unreadable.contains(agent)
    }

    /// The stamps `agent`'s verdict holds for.
    #[cfg(test)]
    pub(crate) fn held_stamps(&self, agent: &AgentId) -> Option<Vec<Stamp>> {
        lock(&self.inner)
            .agents
            .get(agent)
            .and_then(|entry| entry.stamps.clone())
    }

    /// `agent`'s recorded account.
    #[cfg(test)]
    pub(crate) fn recorded(&self, agent: &AgentId) -> Option<AccountHash> {
        lock(&self.inner).recorded.get(agent).cloned()
    }
    /// A sign-in or reuse through Clauseo starts for `agent` at `now`, the login files at
    /// `stamps`: its `SignedIn` records, until `now + bound` (the sign-in run's own).
    pub(crate) fn expect(
        &self,
        agent: &AgentId,
        now: Duration,
        bound: Duration,
        stamps: Vec<Stamp>,
    ) {
        self.entry(agent, |entry| {
            entry.expect = Some(now + bound);
            entry.pending = Some(stamps);
        });
    }

    /// S18 reports `agent`'s sign-in still running at `now`, the login files at `stamps`: a
    /// pending one's bound moves on, and its record will hold for these stamps.
    pub(crate) fn expect_running(
        &self,
        agent: &AgentId,
        now: Duration,
        bound: Duration,
        stamps: Vec<Stamp>,
    ) {
        self.entry(agent, |entry| {
            if entry.expect.is_some() {
                entry.expect = Some(now + bound);
                entry.pending = Some(stamps);
            }
        });
    }

    /// The call that would have signed `agent` in was refused: nothing is under way.
    pub(crate) fn expect_none(&self, agent: &AgentId) {
        // Held, it stays due, and the next look checks: its files moved.
        self.entry(agent, |entry| {
            let _held = entry.expect_end();
        });
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
    /// `Same`, or `Unknown` with no account, from `now`. The stamps it holds for are those
    /// taken before the status read that named it: `stamps` when given (a sign-in's), else
    /// those the entry has (a check's, taken as it began); never cleared.
    fn record(
        &self,
        agent: &AgentId,
        hash: Option<AccountHash>,
        now: Duration,
        stamps: Option<Vec<Stamp>>,
    ) -> Verdict {
        let mut inner = lock(&self.inner);
        inner.unreadable.remove(agent);
        inner.nameless.remove(agent);
        inner.signed_out.remove(agent);
        let verdict = match hash {
            Some(hash) => {
                inner.recorded.insert(agent.clone(), hash);
                Verdict::Same
            }
            None => Verdict::Unknown,
        };
        let entry = inner.agents.entry(agent.clone()).or_default();
        entry.verdict = verdict;
        entry.again = false;
        let _held = entry.expect_end();
        if stamps.is_some() {
            entry.stamps = stamps;
        }
        entry.checked_at = Some(now);
        entry.generation += 1;
        let recorded = inner.recorded.clone();
        drop(inner);
        if let Err(error) = self.store.save(&recorded) {
            tracing::warn!(%agent, kind = ?error.kind(), "the identity record was not written");
        }
        verdict
    }

    /// A pending sign-in's stamps, taken before its confirming status read.
    fn pending_stamps(&self, agent: &AgentId) -> Option<Vec<Stamp>> {
        lock(&self.inner)
            .agents
            .get(agent)
            .and_then(|entry| entry.pending.clone())
    }

    /// The verdict of `read` against `agent`'s record. With none recorded, an account is
    /// trusted on first use only while no launch of the agent has passed the launch gate (after
    /// one it is unknown), and no account means nothing signed in: idle.
    fn judge(&self, agent: &AgentId, read: &AccountRead) -> Judged {
        let inner = lock(&self.inner);
        if inner.unreadable.contains(agent) {
            return Judged::Is(Verdict::Unknown);
        }
        let launched = inner.agents.get(agent).is_some_and(|entry| entry.launched);
        match (read, inner.recorded.get(agent)) {
            (AccountRead::Account(hash), Some(recorded)) if hash == recorded => {
                Judged::Is(Verdict::Same)
            }
            (AccountRead::Account(_), Some(_)) => Judged::Is(Verdict::Changed),
            (AccountRead::Account(_), None) if launched => Judged::Is(Verdict::Unknown),
            (AccountRead::Account(hash), None) => Judged::Record(hash.clone()),
            (AccountRead::NoAccount, None) if !inner.nameless.contains(agent) => {
                Judged::Is(Verdict::Idle)
            }
            (AccountRead::NoAccount, Some(_)) if inner.signed_out.contains(agent) => {
                Judged::Is(Verdict::SignedOut)
            }
            _ => Judged::Is(Verdict::Unknown),
        }
    }

    /// Starts a check of `agent` at `now` with the login files at `stamps`: `None` (no check)
    /// when one runs, its account changed, or a sign-in through Clauseo is under way.
    fn begin(&self, agent: &AgentId, stamps: Vec<Stamp>, now: Duration) -> Option<Ticket> {
        self.entry(agent, |entry| {
            let checkable = !matches!(entry.verdict, Verdict::Checking | Verdict::Changed);
            if !checkable || entry.expecting(now) {
                return None;
            }
            entry.verdict = Verdict::Checking;
            entry.stamps = Some(stamps);
            Some(Ticket(entry.generation))
        })
    }

    /// A credential in the launch env replaces the shared login for `agent` (`exempt`): any
    /// verdict but a changed account becomes [`Verdict::KeyLogin`], and a check in flight is
    /// dropped (the generation moves on). Whether it changed now, for the log.
    fn key_login(&self, agent: &AgentId) -> bool {
        self.entry(agent, |entry| {
            let checked = !matches!(entry.verdict, Verdict::KeyLogin | Verdict::Changed);
            if checked {
                entry.verdict = Verdict::KeyLogin;
                entry.generation += 1;
            }
            checked
        })
    }

    /// The launch env no longer replaces `agent`'s shared login: a key login is due a check,
    /// before anything launches on the login (a check in flight is dropped). Whether it changed.
    fn key_login_ended(&self, agent: &AgentId) -> bool {
        self.entry(agent, |entry| {
            let ended = entry.verdict == Verdict::KeyLogin;
            if ended {
                entry.verdict = Verdict::Due;
                entry.generation += 1;
            }
            ended
        })
    }

    /// A sign-in through Clauseo whose `SignedIn` names no email: the account it replaced is no
    /// longer recorded, and `agent` is due a check, whose status may name it by its id; the
    /// check's first account is then recorded, so the new login never reads as a swap.
    fn forget(&self, agent: &AgentId, now: Duration) {
        let mut inner = lock(&self.inner);
        inner.unreadable.remove(agent);
        inner.recorded.remove(agent);
        inner.nameless.insert(agent.clone());
        inner.signed_out.remove(agent);
        let entry = inner.agents.entry(agent.clone()).or_default();
        entry.verdict = Verdict::Due;
        let _held = entry.expect_end();
        entry.checked_at = Some(now);
        entry.generation += 1;
        let recorded = inner.recorded.clone();
        drop(inner);
        if let Err(error) = self.store.save(&recorded) {
            tracing::warn!(%agent, kind = ?error.kind(), "the identity record was not written");
        }
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
