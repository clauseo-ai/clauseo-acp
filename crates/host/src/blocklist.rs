//! The unsafe-site block list's update path (ADR 0053): the `clauseo-blocklist` thread loads
//! the newer verified one of the stored and the bundled snapshot at start, then fetches the
//! published pair every interval over the host's own network, verifies it, applies the rollback
//! rule, stores it and swaps it into the shared [`Current`] the egress proxy reads.
//!
//! - The thread is its own `std::thread`, never the actor's pool. It starts once the host
//!   serves (`actor.rs`'s `run`) and stops when the host thread ends: the [`Running`] guard's
//!   drop disconnects its stop channel, and every wait is a `recv_timeout` on that channel, so
//!   it ends at its next wait. Nothing joins it, so a fetch in flight (at most the client's
//!   timeouts) never holds up the host's stop, and a round that sees the stop once its files
//!   are in applies and writes nothing (`Updater::run_once`).
//! - The first fetch runs right after the start-up load, then one every interval, jittered,
//!   with one short retry after a signature mismatch (`timing.rs`).
//! - Each round reads the settings as they are then ([`Live`], which the actor refreshes on
//!   every settings change): `network.unsafe_site_list_updates` off skips the fetch (the list
//!   in use stays enforced), and `network.upstream_proxy` routes it (`fetch.rs`).
//! - Anything that fails keeps the list in use and retries at the next interval; a 404 means
//!   nothing is published yet and is logged at info. A list dated over a day ahead of the host
//!   clock is refused; a list in use over 3 days old is logged once a day
//!   ([`block_list_is_stale`] tells whoever shows it).
//! - Storage (`disk.rs`): `<data_dir>/blocklist/` holds the last good pair, each file replaced
//!   atomically, the list first and its signature last; a crash between the two leaves a pair
//!   that fails verification, so the next start uses the bundled snapshot and the next fetch
//!   restores the pair. No mismatched pair is ever read as good.
//! - Redirects (`fetch.rs`): followed by hand, at most five hops, each resolved against the URL
//!   before it, `https` only (when asked) and to an allowed host only.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use clauseo_blocklist::{Current, MAX_SIGNATURE, PUBLIC_KEYS};
use clauseo_protocol::Timestamp;
use clauseo_sandbox::Upstream;

use crate::clock::Clock;

mod disk;
mod fetch;
mod live;
mod timing;
mod updater;

pub(crate) use live::{Fetching, Live};
pub(crate) use updater::{Round, Updater};

/// The thread's name.
const THREAD_NAME: &str = "clauseo-blocklist";

/// The published list, its signature beside it.
const RELEASE_LIST_URL: &str =
    "https://github.com/clauseo-ai/blocklist/releases/download/current/blocklist-v1.txt.gz";

/// The largest `.gz` the release path reads.
const RELEASE_MAX_LIST: u64 = 64 * 1024 * 1024;

/// How often the release path fetches.
const RELEASE_INTERVAL: Duration = Duration::from_hours(6);

/// A day, in seconds.
pub(crate) const DAY: i64 = 24 * 60 * 60;

/// How old the list in use may get before it counts as stale.
const STALE_AFTER: i64 = 3 * DAY;

/// What the host needs for the block list: the shared list in use, the bundled snapshot's
/// folder and the update settings. Without it ([`crate::HostConfig::with_blocklist`] not
/// called) the host starts no thread and fetches nothing.
pub struct BlockListSeams {
    pub(crate) current: Arc<Current>,
    pub(crate) bundled: Option<PathBuf>,
    pub(crate) update: BlockListUpdate,
}

impl BlockListSeams {
    /// `current`, which the proxy reads too; `bundled`, the folder holding the app's snapshot
    /// (`LIST_FILE` and `SIGNATURE_FILE`), if any; and the update settings.
    pub fn new(current: Arc<Current>, bundled: Option<PathBuf>, update: BlockListUpdate) -> Self {
        Self {
            current,
            bundled,
            update,
        }
    }

    /// [`Self::new`] with [`BlockListUpdate::release`].
    pub fn release(current: Arc<Current>, bundled: Option<PathBuf>) -> Self {
        Self::new(current, bundled, BlockListUpdate::release())
    }

    /// The shared list in use, for whatever else reads it (the egress proxy).
    pub fn current(&self) -> &Arc<Current> {
        &self.current
    }
}

/// Where and how the list is fetched.
#[derive(Debug, Clone)]
pub struct BlockListUpdate {
    /// The `.gz`'s URL.
    pub list_url: String,
    /// The `.minisig`'s URL.
    pub signature_url: String,
    /// The hosts a URL (the first or a redirect's) may name, compared without case.
    pub allowed_hosts: Vec<String>,
    /// Whether plain `http` is refused (and an explicit port).
    pub https_only: bool,
    /// The route when `network.upstream_proxy` is unset: the release asks the system
    /// ([`Upstream::System`]); tests go direct.
    pub upstream: Upstream,
    /// The wait between fetches, before its jitter.
    pub interval: Duration,
    /// The most bytes of a `.gz` read, fetched or from disk.
    pub max_list: u64,
    /// The most bytes of a `.minisig` read.
    pub max_signature: u64,
    /// The minisign key lines a list may be signed with.
    pub keys: Vec<String>,
}

impl BlockListUpdate {
    /// The published release: GitHub's `current` release of `clauseo-ai/blocklist`, which
    /// redirects to `release-assets.githubusercontent.com` (`objects.githubusercontent.com`
    /// before), `https` only, every 6 hours (± 30 min), [`PUBLIC_KEYS`], routed through
    /// `network.upstream_proxy` when set, else the system's proxy (env, then the OS's settings).
    pub fn release() -> Self {
        Self {
            list_url: RELEASE_LIST_URL.to_owned(),
            signature_url: format!("{RELEASE_LIST_URL}.minisig"),
            allowed_hosts: [
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
            ]
            .map(str::to_owned)
            .to_vec(),
            https_only: true,
            upstream: Upstream::System,
            interval: RELEASE_INTERVAL,
            max_list: RELEASE_MAX_LIST,
            max_signature: u64::try_from(MAX_SIGNATURE).unwrap_or(u64::MAX),
            keys: PUBLIC_KEYS.iter().map(|key| (*key).to_owned()).collect(),
        }
    }
}

/// Whether the list in `current` was made over 3 days before `now` (an unloaded list is not
/// stale: it is reported apart). The updater logs it once a day; a notice may ask it too.
pub fn block_list_is_stale(current: &Current, now: Timestamp) -> bool {
    current
        .generated()
        .is_some_and(|generated| now.as_millis() / 1000 - generated.unix_seconds() > STALE_AFTER)
}

/// What the thread starts from, taken out of the host's config.
pub(crate) struct Start {
    pub(crate) seams: BlockListSeams,
    pub(crate) data_dir: PathBuf,
    /// The settings each round reads; the actor keeps a clone current.
    pub(crate) live: Live,
    pub(crate) clock: Arc<dyn Clock>,
}

/// Takes the block list's seams out of `config`, with what the thread needs beside them.
pub(crate) fn take(config: &mut crate::HostConfig) -> Option<Start> {
    let seams = config.blocklist.take()?;
    Some(Start {
        seams,
        data_dir: config.effective.data_dir().to_path_buf(),
        live: Live::new(Fetching::of(config.effective.settings())),
        clock: Arc::clone(&config.clock),
    })
}

/// Puts the actor's settings into the updater's [`Live`], if there is an updater.
pub(crate) fn refresh(actor: &crate::actor::Actor) {
    if let Some(live) = &actor.blocklist {
        live.set(Fetching::of(actor.effective.settings()));
    }
}

/// The running thread; dropping it stops the thread at its next wait.
pub(crate) struct Running {
    _stop: mpsc::Sender<()>,
}

/// Starts the thread; `None` (logged) when the OS refused a thread, and the host runs on with
/// whatever list the seams' `current` holds.
pub(crate) fn start(start: Start) -> Option<Running> {
    let updater = Updater::new(start.seams, &start.data_dir, start.live, start.clock);
    spawn(updater, |_round| {})
        .map_err(|error| tracing::warn!(kind = ?error.kind(), "no block list thread"))
        .ok()
}

/// Starts the thread running `updater`, telling `on_round` each round's outcome.
fn spawn(
    updater: Updater,
    on_round: impl FnMut(Round) + Send + 'static,
) -> std::io::Result<Running> {
    let (stop, stopped) = mpsc::channel::<()>();
    let _detached = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run(&updater, &stopped, on_round))?;
    Ok(Running { _stop: stop })
}

/// The thread's body: the start-up load, a fetch, then one per (jittered) interval until
/// stopped.
fn run(updater: &Updater, stopped: &mpsc::Receiver<()>, mut on_round: impl FnMut(Round)) {
    updater.load_at_start();
    let mut wait = Duration::ZERO;
    let mut retried = false;
    loop {
        match stopped.recv_timeout(wait) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
        let stop_asked = || {
            matches!(
                stopped.try_recv(),
                Ok(()) | Err(mpsc::TryRecvError::Disconnected)
            )
        };
        let round = updater.run_once(&stop_asked);
        on_round(round);
        (wait, retried) = timing::next_wait(updater.interval(), round, retried, timing::random());
    }
}

#[cfg(test)]
mod tests;
