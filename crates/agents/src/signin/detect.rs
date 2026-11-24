//! Detection (the brief, "Detection"): whether the lawyer already has a Claude Code or Codex
//! login. It starts no agent CLI, so it writes nothing into the lawyer's own config and sends
//! nothing over the network: for each agent it is one stat (never an open) of the login file
//! where the CLI keeps it by default (`super::reuse`, the one file that may name them), and on
//! macOS, for Claude, an attributes-only `security find-generic-password` query of Claude's
//! default Keychain item: never `-w`/`-g`; exit status only.
//!
//! Why no CLI: Claude Code's `auth status` writes into the lawyer's folders, even offline. On an
//! expired token it takes and removes its refresh locks (`~/.claude/.oauth_refresh.lock`, an
//! `.owner` record written through a temp file and a rename, and `~/.claude.lock`), and online
//! the refresh rewrites `~/.claude/.credentials.json`; with no `CLAUDE_CONFIG_DIR` it also
//! rewrites `~/.claude.json` and writes a backup the first time a version meets the home. Codex's
//! `login status` writes `~/.codex/tmp/arg0` even when signed out. (Decision 0046, "Detection".)
//!
//! - Claude: `~/.claude/.credentials.json` (`%USERPROFILE%\.claude` on Windows) a regular file
//!   is `LoginFound { from: ClaudeCode, email: None }`; anything else (missing, a link, a folder)
//!   is `SignedOut { found: NoLogin }`. On macOS (`config.platform`, decided at run time),
//!   where Claude Code keeps the login in the Keychain, the login is also found when
//!   `/usr/bin/security find-generic-password -s "Claude Code-credentials"` exits 0
//!   (`reuse::claude_keychain`: attributes only, output discarded, killed past 3 s). 44 is no
//!   login; any other end could not tell, which is no login at first detection and keeps a
//!   recorded reuse (`found.rs`). Its answer is kept for 10 minutes ([`KeychainMemo`]), since a
//!   locked keychain may show its unlock dialog for the query: so at most one dialog per 10
//!   minutes, and "Look for my Claude Code login" asks afresh. When the lawyer's environment
//!   moves Claude's store ([`claude_store_moved`]) nothing is found and no query runs: their
//!   Claude Code keeps its login elsewhere (a folder of its own, a suffixed Keychain item), and
//!   "Use it" reads the default store only (`CLAUDE_SECURESTORAGE_CONFIG_DIR=""`).
//! - Codex: `~/.codex/auth.json` a regular file is `LoginFound { from: Codex, email: None }`, else
//!   `SignedOut { found: NoLogin }`. With `CODEX_HOME` set in the lawyer's environment nothing is
//!   found: "Use it", the split check and the guard address `~/.codex` only.
//!
//! No email, plan or account comes from detection. "Use it" confirms through the agent's own
//! status under the reused environment (the lawyer's explicit choice, so a refresh write there is
//! theirs), which gives the plan and what email it reports. Nothing ties a reuse to an account:
//! the agent runs as the lawyer, and switching accounts is their own doing.
//!
//! Detection never starts reuse: it gives `LoginFound`, and only "Use it" reuses. It runs in each
//! status check and again at each Sign in press ([`at_press`]), so nothing it saw is kept.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::Timer;

use crate::cancel::Cancel;
use crate::platform::Os;
use crate::signin::env::Vendor;
use crate::signin::job::{SigninJob, Task};
use crate::signin::reuse::KeychainSeen;
use crate::signin::{Method, SigninState, marker, reuse};
use crate::status::{Identity, claude_detection, codex_detection};

/// What detection found: the state, and an account it never knows (kept for the callers'
/// shape: always empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    /// `LoginFound`, or `SignedOut` with what was seen.
    pub(crate) state: SigninState,
    /// Always empty: detection reads no email or plan.
    pub(crate) identity: Identity,
    /// Claude on macOS: no login file, and the Keychain query could not tell (`state` is then
    /// no login). A recorded reuse is kept rather than stopped (`found::check`).
    pub(crate) could_not_tell: bool,
}

/// Claude Code's config folder, which also suffixes its Keychain item unless the next is set.
const CLAUDE_CONFIG: &str = "CLAUDE_CONFIG_DIR";

/// Claude Code's secure-storage folder: set, it decides the store alone; `""` is the default.
const CLAUDE_STORAGE: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

/// How long a Keychain answer is kept: re-checks within it ask no second time.
pub(crate) const KEYCHAIN_RECHECK: Duration = Duration::from_mins(10);

/// Claude's detection, a stat, and on macOS the Keychain query when the stat finds nothing: see
/// the module docs. `None` only when cancelled.
pub(crate) async fn claude<T: Timer>(job: &SigninJob<T>, cancel: &Cancel) -> Option<Detected> {
    let moved = claude_store_moved(&job.config.signin.base_env, cfg!(windows));
    let file = detected(job, cancel, moved, reuse::claude_login_present)?;
    let seen = if file || moved || job.config.platform.os != Os::Darwin {
        None
    } else {
        Some(in_keychain(job).await)
    };
    let present = file || seen == Some(KeychainSeen::Found);
    Some(Detected {
        state: claude_detection(present),
        identity: Identity::default(),
        could_not_tell: seen == Some(KeychainSeen::Unknown),
    })
}

/// Whether Claude Code keeps the lawyer's login somewhere other than the default store
/// (`~/.claude` and the unsuffixed Keychain item), by Claude Code 0.3.330's rule: with
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` set, a folder moves it (`""` is the default store,
/// whatever `CLAUDE_CONFIG_DIR` says); unset, a non-empty `CLAUDE_CONFIG_DIR` moves it.
pub(crate) fn claude_store_moved(env: &BTreeMap<OsString, OsString>, windows: bool) -> bool {
    match value(env, CLAUDE_STORAGE, windows) {
        Some(storage) => !storage.is_empty(),
        None => folder_moved(env, CLAUDE_CONFIG, windows),
    }
}

/// Claude Code's default Keychain item, by the query or by an answer kept from the last
/// [`KEYCHAIN_RECHECK`]; "Look for my … login" always asks afresh.
async fn in_keychain<T: Timer>(job: &SigninJob<T>) -> KeychainSeen {
    let now = job.config.clock.monotonic();
    let memo = &job.shared.keychain;
    if job.task != Task::FindLogin
        && let Some(seen) = memo.fresh(now)
    {
        return seen;
    }
    let signin = &job.config.signin;
    let seen = reuse::claude_keychain(
        &*job.launcher,
        &*job.timer,
        &signin.security_program(),
        &signin.base_env,
    )
    .await;
    memo.keep(now, seen);
    seen
}

/// The last Keychain answer and when it came (the clock's monotonic reading), one per `Agents`.
#[derive(Debug, Default)]
pub(crate) struct KeychainMemo(Mutex<Option<(Duration, KeychainSeen)>>);

impl KeychainMemo {
    /// The kept answer while it is younger than [`KEYCHAIN_RECHECK`] at `now`.
    pub(crate) fn fresh(&self, now: Duration) -> Option<KeychainSeen> {
        let kept = *self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.filter(|(at, _)| {
            now.checked_sub(*at)
                .is_some_and(|age| age < KEYCHAIN_RECHECK)
        })
        .map(|(_, seen)| seen)
    }

    /// Keeps `seen`, asked at `now`.
    pub(crate) fn keep(&self, now: Duration, seen: KeychainSeen) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((now, seen));
    }
}

/// Codex's detection, a stat alone: see the module docs. `None` only when cancelled.
pub(crate) fn codex<T>(job: &SigninJob<T>, cancel: &Cancel) -> Option<Detected> {
    let moved = folder_moved(&job.config.signin.base_env, "CODEX_HOME", cfg!(windows));
    detected(job, cancel, moved, reuse::lawyer_login_present)
        .map(|present| found(codex_detection(present)))
}

/// Whether the agent's login file is there by `stat` (in the lawyer's home), unless the
/// lawyer's environment `moved` the agent's folder; `None` when cancelled.
fn detected<T>(
    job: &SigninJob<T>,
    cancel: &Cancel,
    moved: bool,
    stat: fn(&Path) -> bool,
) -> Option<bool> {
    if cancel.is_cancelled() {
        return None;
    }
    Some(
        !moved
            && job
                .config
                .signin
                .lawyer_home()
                .is_some_and(|lawyer| stat(&lawyer)),
    )
}

/// Sign in pressed with a plan method (not Claude's Console) and Clauseo's own login not signed
/// in (the quick check said so, or could not tell): detection again, so the lawyer's own login is
/// offered ("Use it", `LoginFound`) rather than a browser opened, whatever an earlier status
/// check saw. That check runs once per app start, after the install's first `Ready`, and none
/// runs for a press kept until `Ready`; one that got no answer shows Sign in with nothing
/// detected, and a login file made after it is never seen. `None`, so the sign-in goes on:
/// nothing found, `no-reuse` there (the lawyer chose Clauseo's own sign-in), the login already
/// reused, for Codex any login file in Clauseo's home (Clauseo's own, which "Use it" could not
/// link over, or one shared with the lawyer's: the login's guard decides), or cancelled.
pub(crate) async fn at_press<T: Timer>(job: &SigninJob<T>) -> Option<SigninState> {
    if job.task == Task::SignIn(Method::Console)
        || marker::no_reuse(&job.config.data_dir, &job.ticket.agent)
        || super::found::is_reused(job)
    {
        return None;
    }
    let cancel = &job.ticket.cancel;
    let found = match job.vendor {
        Vendor::Claude => claude(job, cancel).await?,
        Vendor::Codex if std::fs::symlink_metadata(reuse::login_file(&job.home)).is_ok() => {
            return None;
        }
        Vendor::Codex => codex(job, cancel)?,
    };
    matches!(found.state, SigninState::LoginFound { .. }).then_some(found.state)
}

fn found(state: SigninState) -> Detected {
    Detected {
        state,
        identity: Identity::default(),
        could_not_tell: false,
    }
}

/// `name`'s value in `env`, set even when empty (any case on Windows, as Windows reads it).
fn value<'a>(
    env: &'a BTreeMap<OsString, OsString>,
    name: &str,
    windows: bool,
) -> Option<&'a OsStr> {
    env.iter()
        .find(|(key, _)| {
            let key = key.to_string_lossy();
            if windows {
                key.eq_ignore_ascii_case(name)
            } else {
                key == name
            }
        })
        .map(|(_, value)| value.as_os_str())
}

/// Whether the lawyer's environment `env` sets `name` (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) to a
/// folder (not empty; names in any case on Windows, as Windows reads them). Their CLI then keeps
/// its login under that folder, while "Use it" takes the default one only; so detection
/// reports nothing found rather than offer a login "Use it" would not take.
pub(crate) fn folder_moved(env: &BTreeMap<OsString, OsString>, name: &str, windows: bool) -> bool {
    env.iter().any(|(key, value)| {
        let key = key.to_string_lossy();
        let named = if windows {
            key.eq_ignore_ascii_case(name)
        } else {
            key == name
        };
        named && !value.is_empty()
    })
}

#[cfg(test)]
mod tests;
