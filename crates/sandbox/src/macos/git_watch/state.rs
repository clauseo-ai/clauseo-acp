//! The matter's state file, `<git set-aside>/matters/<hash of the matter>.json`, read and
//! written under an advisory lock (`flock`) on a `.lock` file beside it. It holds:
//!
//! - each running session's record: its host process (pid and start), its probe name, its
//!   set-aside folder and its launch baseline. The session's own watch removes it after its
//!   final scan. One whose host is gone, or whose watch is not running in its host, never ran
//!   that scan: the matter's next launch kills what is left of it and runs the scan for it,
//!   from its baseline, before taking anything in the matter for the lawyer's.
//! - the items a final scan could neither move nor neutralise: while one is still there, the
//!   matter's launches are refused with a plain reason.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::aside::{Aside, Stuck};
use super::{Fs, Phase, Watch};
use crate::DenialSink;
use crate::policy::path_key;

/// A file ID: the volume and the inode.
pub(crate) type Id = (u64, u64);

/// A path as its bytes, so any name round-trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Bytes(Vec<u8>);

impl From<&Path> for Bytes {
    fn from(path: &Path) -> Bytes {
        Bytes(path.as_os_str().as_bytes().to_vec())
    }
}

impl From<&PathBuf> for Bytes {
    fn from(path: &PathBuf) -> Bytes {
        Bytes::from(path.as_path())
    }
}

impl Bytes {
    pub(crate) fn path(&self) -> PathBuf {
        PathBuf::from(OsStr::from_bytes(&self.0))
    }
}

/// A path with the file ID it had at launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Known {
    path: Bytes,
    pub(crate) id: Id,
}

impl Known {
    pub(crate) fn new(path: &Path, id: Id) -> Known {
        Known {
            path: path.into(),
            id,
        }
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.path.path()
    }
}

/// What a session's launch knew of the matter, for its final scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Baseline {
    pub(crate) repos: Vec<Known>,
    pub(crate) heads: Vec<Known>,
    pub(crate) links: Vec<Known>,
    pub(crate) common: Vec<(Bytes, Option<Id>)>,
    pub(crate) skip: Vec<Bytes>,
}

/// One running session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionRecord {
    pub(crate) session: String,
    pub(crate) host_pid: u32,
    pub(crate) host_start: Option<u64>,
    pub(crate) probe: String,
    pub(crate) set_aside: Bytes,
    pub(crate) baseline: Baseline,
}

/// An item left in the matter that blocks its launches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StuckRecord {
    path: Bytes,
    reason: String,
}

/// The state file's contents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MatterState {
    /// The matter, for a person reading the file.
    #[serde(default)]
    pub(crate) matter: String,
    #[serde(default)]
    pub(crate) sessions: Vec<SessionRecord>,
    #[serde(default)]
    pub(crate) stuck: Vec<StuckRecord>,
}

/// Where a matter's state lives: the file, or the tests' memory.
pub(crate) trait Store: Send + 'static {
    /// Runs `change` on the state under the matter's lock, then saves it.
    fn update<T>(&self, change: impl FnOnce(&mut MatterState) -> T) -> io::Result<T>;
}

/// What a launch needs to finish dead sessions.
pub(crate) struct Host<'a> {
    /// Whether a recorded session never ran its final scan.
    pub(crate) dead: &'a dyn Fn(&SessionRecord) -> bool,
    /// Ends every process of a dead session by its probe name, waiting a little.
    pub(crate) kill: &'a dyn Fn(&str),
}

/// At launch, under the matter's lock: refuses while an item a final scan left is still in
/// the matter; then kills what is left of each dead session and runs its final scan.
pub(crate) fn begin<F: Fs + Clone, S: Store>(
    store: &S,
    fs: &F,
    matter: &Path,
    notices: &Arc<dyn DenialSink>,
    host: &Host<'_>,
) -> io::Result<()> {
    store.update(|state| {
        state.matter = matter.display().to_string();
        state
            .stuck
            .retain(|stuck| fs.meta(&stuck.path.path()).is_some());
        if let Some(stuck) = state.stuck.first() {
            let shown = stuck.path.path();
            let shown = shown.strip_prefix(matter).unwrap_or(&shown);
            return Err(io::Error::other(format!(
                "Clauseo won't start an AI tool in this matter until {} is moved out of it: an \
                 earlier AI tool left it there, and Clauseo could neither move nor rename it \
                 ({}).",
                shown.display(),
                stuck.reason
            )));
        }
        let (dead, live): (Vec<_>, Vec<_>) = std::mem::take(&mut state.sessions)
            .into_iter()
            .partition(|record| (host.dead)(record));
        state.sessions = live;
        for record in dead {
            tracing::warn!(
                session = record.session, matter = %matter.display(),
                "a session of this matter never ran its final git scan; running it now"
            );
            (host.kill)(&record.probe);
            let aside = Aside::new(
                matter.to_owned(),
                record.set_aside.path(),
                record.session.clone(),
                Arc::clone(notices),
            );
            let mut watch = Watch::resume(fs.clone(), matter.to_owned(), aside, record.baseline);
            let asks = watch.full_scan(Phase::Ended);
            debug_assert!(!asks.end, "a final scan never asks to end");
            add_stuck(state, watch.take_stuck());
        }
        Ok(())
    })?
}

/// Records session `record` as running.
pub(crate) fn record<S: Store>(store: &S, record: SessionRecord) -> io::Result<()> {
    store.update(|state| state.sessions.push(record))
}

/// After session `session`'s final scan: its record goes, and what it left `stuck` stays.
pub(crate) fn end<S: Store>(store: &S, session: &str, stuck: Vec<Stuck>) {
    let saved = store.update(|state| {
        state.sessions.retain(|record| record.session != session);
        add_stuck(state, stuck);
    });
    if let Err(error) = saved {
        tracing::error!(%error, session, "the git watch could not update the matter's state");
    }
}

fn add_stuck(state: &mut MatterState, stuck: Vec<Stuck>) {
    state
        .stuck
        .extend(stuck.into_iter().map(|stuck| StuckRecord {
            path: stuck.path.as_path().into(),
            reason: stuck.reason,
        }));
}

/// The matter's state file in the data dir's git set-aside folder.
pub(crate) struct FileStore {
    file: PathBuf,
    lock: PathBuf,
}

impl FileStore {
    /// The state of `matter`, kept under `set_aside`.
    pub(crate) fn new(set_aside: &Path, matter: &Path) -> FileStore {
        let hash = Sha256::digest(path_key(matter).as_os_str().as_bytes());
        let mut name = String::new();
        for b in hash.iter().take(16) {
            name.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
            name.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
        }
        let dir = set_aside.join("matters");
        FileStore {
            file: dir.join(format!("{name}.json")),
            lock: dir.join(format!("{name}.lock")),
        }
    }
}

impl Store for FileStore {
    fn update<T>(&self, change: impl FnOnce(&mut MatterState) -> T) -> io::Result<T> {
        let _lock = super::os::lock(&self.lock)?;
        let mut state = match super::os::read_small(&self.file) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                io::Error::other(format!("{} can't be read: {error}", self.file.display()))
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => MatterState::default(),
            Err(error) => return Err(error),
        };
        let out = change(&mut state);
        let bytes = serde_json::to_vec_pretty(&state).map_err(io::Error::other)?;
        super::os::replace(&self.file, &bytes)?;
        Ok(out)
    }
}
