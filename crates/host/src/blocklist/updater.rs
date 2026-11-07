//! The work of the `clauseo-blocklist` thread, one step at a time so tests drive it: the
//! start-up load ([`Updater::load_at_start`]) and one fetch round ([`Updater::run_once`]).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_blocklist::{BlockList, Current, Error, Generated};
use clauseo_sandbox::Upstream;

use super::disk::{self, Pair};
use super::fetch::{self, Clients, Failure, fetch};
use super::{BlockListSeams, BlockListUpdate, DAY, Live, block_list_is_stale};
use crate::clock::Clock;

/// The folder under the data folder holding the stored pair.
const STORE_FOLDER: &str = "blocklist";

/// The signature's whole-request timeout: the connectors' 60 s.
const SIGNATURE_TIMEOUT: Duration = Duration::from_mins(1);

/// The list's whole-request timeout: up to 64 MiB on a slow link.
const LIST_TIMEOUT: Duration = Duration::from_mins(10);

/// How one fetch round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Round {
    /// A newer verified list is in use (and stored, unless the write failed: logged).
    Applied,
    /// A verified list that the rollback rule did not take: not newer.
    NotNewer,
    /// A 404 on either file: nothing published yet.
    NotPublished,
    /// The signature did not verify (the release may be between its two files); retried soon.
    Mismatch,
    /// Anything else; the list in use stays.
    Failed,
    /// Updates are off by policy: no request.
    Off,
    /// The host stopped during the round: nothing applied, nothing written.
    Stopped,
}

/// The updater's state: the list in use, where the pairs are, and the bundled snapshot's time
/// (the rollback rule's floor), known once the start-up load has run.
pub(crate) struct Updater {
    current: Arc<Current>,
    bundled_folder: Option<PathBuf>,
    store_folder: PathBuf,
    update: BlockListUpdate,
    clients: Clients,
    /// The settings each round reads.
    live: Live,
    /// The host's clock: the list's date checks.
    clock: Arc<dyn Clock>,
    bundled_time: std::sync::OnceLock<Option<Generated>>,
    /// Whether "updates are off" was logged since they last were on.
    off_noted: Cell<bool>,
    /// When (Unix seconds) "the list is stale" was last logged.
    stale_noted: Cell<Option<i64>>,
}

impl Updater {
    /// The updater of `seams`, storing under `data_dir`, reading `live` each round.
    pub(crate) fn new(
        seams: BlockListSeams,
        data_dir: &Path,
        live: Live,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let clients = Clients::new(seams.update.https_only);
        Self {
            current: seams.current,
            bundled_folder: seams.bundled,
            store_folder: data_dir.join(STORE_FOLDER),
            update: seams.update,
            clients,
            live,
            clock,
            bundled_time: std::sync::OnceLock::new(),
            off_noted: Cell::new(false),
            stale_noted: Cell::new(None),
        }
    }

    /// The wait between rounds.
    pub(crate) fn interval(&self) -> Duration {
        self.update.interval
    }

    /// Loads and verifies the bundled and the stored pair, and offers the stored one, then the
    /// bundled one, with the bundled time as the floor: the newer verified one is in use, and a
    /// stored copy older than the bundled one is ignored. With neither the list stays unloaded.
    pub(crate) fn load_at_start(&self) {
        let bundled = self
            .bundled_folder
            .as_deref()
            .and_then(|folder| self.load_from(folder, "bundled"));
        let floor = bundled.as_ref().and_then(BlockList::generated);
        // Only this thread sets it, once; a second call keeps the first floor.
        let floor = *self.bundled_time.get_or_init(|| floor);
        if let Some(stored) = self.load_from(&self.store_folder, "stored") {
            self.current.offer(stored, floor);
        }
        if let Some(bundled) = bundled {
            self.current.offer(bundled, floor);
        }
        let list = self.current.get();
        if let Some(generated) = list.generated() {
            tracing::info!(
                generated = generated.as_str(),
                entries = list.len(),
                "the block list is loaded"
            );
        } else {
            tracing::info!(
                "no block list snapshot (a dev build?): no site is blocked until a fetch succeeds"
            );
        }
    }

    /// The verified list in `folder`, or `None` (logged when the files are there but refused).
    fn load_from(&self, folder: &Path, which: &'static str) -> Option<BlockList> {
        let pair = match disk::read_pair(folder, self.update.max_list, self.update.max_signature) {
            Ok(Some(pair)) => pair,
            Ok(None) => return None,
            Err(error) => {
                tracing::warn!(which, kind = ?error.kind(), "a block list snapshot is unreadable");
                return None;
            }
        };
        match clauseo_blocklist::load(&pair.gz, &pair.signature, &self.keys()) {
            // Set aside, as a fetched one is refused: in use, it would hold back every later list.
            Ok(list) if self.ahead(&list) => {
                tracing::warn!(
                    which,
                    generated = list.generated().as_ref().map(Generated::as_str),
                    "a block list snapshot is dated over a day ahead of this computer's clock; \
                     set aside"
                );
                None
            }
            Ok(list) => Some(list),
            Err(error) => {
                tracing::warn!(which, %error, "a block list snapshot was refused");
                None
            }
        }
    }

    /// One round: fetch the signature and the list, verify them, offer the list, and store the
    /// pair when it went into use. Whatever fails keeps the list in use.
    ///
    /// `stopped` is asked once both files are in, before anything is applied or written: once
    /// the host has stopped (an app data deletion may follow), the round writes nothing. What
    /// remains: a GET in flight when the host stops runs on until it ends or times out (60 s for
    /// the signature, [`LIST_TIMEOUT`] for the list), and then the round ends unwritten.
    pub(crate) fn run_once(&self, stopped: &dyn Fn() -> bool) -> Round {
        let round = self.round(stopped);
        self.note_stale();
        round
    }

    /// [`Self::run_once`] but the stale check.
    fn round(&self, stopped: &dyn Fn() -> bool) -> Round {
        let fetching = self.live.get();
        if !fetching.updates {
            if !self.off_noted.replace(true) {
                tracing::info!(
                    "block list updates are off by firm policy; using the stored or bundled list"
                );
            }
            return Round::Off;
        }
        self.off_noted.set(false);
        let fetched = fetch::upstream(fetching.upstream_proxy.as_deref(), &self.update.upstream)
            .and_then(|upstream| self.fetch_pair(&upstream));
        let pair = match fetched {
            Ok(pair) => pair,
            Err(Failure::NotPublished) => {
                tracing::info!("no block list published yet");
                return Round::NotPublished;
            }
            Err(Failure::Refused(why)) => {
                tracing::info!(%why, "the block list fetch failed; the list in use stays");
                return Round::Failed;
            }
        };
        if stopped() {
            return Round::Stopped;
        }
        let list = match clauseo_blocklist::load(&pair.gz, &pair.signature, &self.keys()) {
            Ok(list) => list,
            Err(error) => {
                tracing::warn!(%error, "the fetched block list was refused; the list in use stays");
                return if matches!(error, Error::Signature) {
                    Round::Mismatch
                } else {
                    Round::Failed
                };
            }
        };
        let (generated, entries) = (list.generated(), list.len());
        if self.ahead(&list) {
            tracing::warn!(
                generated = generated.as_ref().map(Generated::as_str),
                "the fetched block list is dated over a day ahead of this computer's clock; \
                 the list in use stays"
            );
            return Round::Failed;
        }
        let floor = self.bundled_time.get().copied().flatten();
        if !self.current.offer(list, floor) {
            return Round::NotNewer;
        }
        tracing::info!(
            generated = generated.as_ref().map(Generated::as_str),
            entries,
            "the block list is updated"
        );
        if let Err(error) = disk::store_pair(&self.store_folder, &pair) {
            tracing::warn!(kind = ?error.kind(), "the new block list could not be stored");
        }
        Round::Applied
    }

    /// Logs at warn, at most once a day, while the list in use is over 3 days old.
    fn note_stale(&self) {
        let now = self.now();
        let wall = self.clock.now().wall;
        if !block_list_is_stale(&self.current, wall) {
            self.stale_noted.set(None);
            return;
        }
        if self
            .stale_noted
            .get()
            .is_some_and(|noted| now - noted < DAY)
        {
            return;
        }
        self.stale_noted.set(Some(now));
        tracing::warn!(
            generated = self.current.generated().as_ref().map(Generated::as_str),
            "the block list in use is over 3 days old"
        );
    }

    /// When (Unix seconds) the stale warning was last logged: for tests.
    #[cfg(test)]
    pub(crate) fn stale_noted(&self) -> Option<i64> {
        self.stale_noted.get()
    }

    /// Whether `list` is dated more than a day ahead of the host clock.
    fn ahead(&self, list: &BlockList) -> bool {
        list.generated()
            .is_some_and(|time| time.unix_seconds() > self.now() + DAY)
    }

    /// The host clock's wall time, in Unix seconds.
    fn now(&self) -> i64 {
        self.clock.now().wall.as_millis() / 1000
    }

    /// The signature, then the list, through `upstream`.
    fn fetch_pair(&self, upstream: &Upstream) -> Result<Pair, Failure> {
        let update = &self.update;
        let clients = (&self.clients, upstream);
        let signature = (update.max_signature, SIGNATURE_TIMEOUT);
        let signature = fetch(clients, &update.signature_url, update, signature)
            .map_err(|failure| named(failure, "signature"))?;
        let list = (update.max_list, LIST_TIMEOUT);
        let gz = fetch(clients, &update.list_url, update, list)
            .map_err(|failure| named(failure, "list"))?;
        Ok(Pair { gz, signature })
    }

    /// The keys as `verify` takes them.
    fn keys(&self) -> Vec<&str> {
        self.update.keys.iter().map(String::as_str).collect()
    }
}

/// `failure`, saying which file it was about.
fn named(failure: Failure, file: &str) -> Failure {
    match failure {
        Failure::NotPublished => Failure::NotPublished,
        Failure::Refused(why) => Failure::Refused(format!("the {file}: {why}")),
    }
}
