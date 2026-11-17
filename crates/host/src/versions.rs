//! The version-history tool, `clauseo-versions`: two read-only MCP tools the lawyer's agent gets
//! beside the undo note, answered by the host against one session's matter. This part answers
//! one MCP message; the listener that carries the messages, its token file and the profile entry
//! are the transport's.
//!
//! - [`VersionsBackend::new`] takes a [`VersionsConfig`]: the session's [`SessionKey`], set by
//!   the host at launch and never by the agent, and the [`VersionsSource`] it reads the matter
//!   through. [`VersionsBackend::answer`] answers one JSON-RPC message (`initialize`, `ping`,
//!   `tools/list`, `tools/call`); a notification or a response gets nothing back.
//! - [`VersionsBackend::turns`] gives the [`VersionsTurns`] the host moves when a turn starts and
//!   ends, as the case-law relay's `TurnHandle`: each turn (and each stretch between turns) may
//!   make [`MAX_CALLS_PER_TURN`] calls, of which [`MAX_DOCX_RUNS_PER_TURN`] may run stetdoc.
//! - `list_versions` (`list.rs`): this matter's versions, newest first, with the files each
//!   changed, read from history alone by `Matter::changes_between`, [`VERSIONS_PER_JOB`] diffs
//!   per job on the matter's thread so a checkpoint never waits behind a whole page.
//! - `read_version` (`read.rs`): one version's file as text, a page of [`PAGE_BYTES`] at a time,
//!   after these checks in order: the version is on this matter's timeline (another matter's id
//!   is unknown here, refused as a made-up one is); the path is in that version's tree, as
//!   `Matter::files_at` lists it, excluded paths left out (`clauseo_matters::version_file`), so
//!   an old `.claude` file or anything outside the folder is never read; unless
//!   [`DELETED_FILES_READABLE`], the file stayed in the matter from that version on
//!   (`continuity.rs`) and is in the folder now (`clauseo_matters::holds_now`); its format is
//!   text (UTF-8, no NUL, at most [`MAX_TEXT_BYTES`]) or Word (`.docx`, at most
//!   [`MAX_DOCX_BYTES`], as `stetdoc view` shows it, `docx.rs`).
//! - The matter is read only when its thread already holds it open: a read never opens or
//!   relinks it. One job per session is on the matter's thread at a time (`flight.rs`), and a
//!   job whose caller gave up does nothing. One stetdoc run per session at a time.
//! - Nothing is written in the matter folder or its history. A Word file is copied (capped at
//!   the matter's size cap) to a private folder under `<data>/tmp/versions-<id>` for stetdoc and
//!   removed when its run ends (a crash leaves one the blackline's start-up sweep removes).
//!
//! Every refusal is one plain sentence (`texts.rs`) the agent can repeat to the lawyer. Logs name
//! the session, the tool and the refusal, never a path, an argument or a file's text.

mod caps;
mod continuity;
mod docx;
mod flight;
mod list;
mod mcp;
mod read;
pub(crate) mod source;
mod texts;
mod window;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use clauseo_matters::Matter;
use clauseo_protocol::{CheckpointId, MatterId, Timestamp};
use serde_json::Value;

use crate::clock::Clock;
use crate::config::SessionKey;

pub(crate) use docx::DIR_PREFIX as DOCX_DIR_PREFIX;

/// The server's name in the agent's MCP list: Claude names the tools
/// `mcp__clauseo-versions__list_versions`, Codex `mcp.clauseo-versions.list_versions`.
pub(crate) const SERVER_NAME: &str = "clauseo-versions";

/// Whether an earlier version of a file the matter no longer holds may be read. Off: such a
/// file is listed in its version's changes, but `read_version` refuses it, and the lawyer can
/// bring it back from Versions. Old versions of files the matter still holds are readable either
/// way, text the lawyer later removed included.
pub(crate) const DELETED_FILES_READABLE: bool = false;

/// The most `tools/call`s one turn (or one stretch between turns) may make.
pub(crate) const MAX_CALLS_PER_TURN: u32 = 100;

/// The most stetdoc runs one turn (or one stretch between turns) may start. Each may take up to
/// 10 s; twenty bound a looping agent to about 200 s of stetdoc a turn while leaving room to read
/// a few Word files across several versions. Paging one Word file runs stetdoc once: its text is
/// kept for the next page.
pub(crate) const MAX_DOCX_RUNS_PER_TURN: u32 = 20;

/// Versions `list_versions` gives when `limit` is not given.
pub(crate) const DEFAULT_LIMIT: usize = 20;

/// The most versions one `list_versions` gives.
pub(crate) const MAX_LIMIT: usize = 50;

/// The most file names one version lists; `files_total` counts them all.
pub(crate) const MAX_FILES_PER_VERSION: usize = 25;

/// The most text bytes one `read_version` page holds (about 16k tokens).
pub(crate) const PAGE_BYTES: usize = 64 * 1024;

/// Tree diffs per job on the matter's thread: a checkpoint queued behind a `list_versions`
/// waits for at most this many.
pub(crate) const VERSIONS_PER_JOB: usize = 10;

/// Timeline entries the deleted-file limit looks a path up in per job on the matter's thread
/// (one tree lookup each, no diff unless the path is missing there).
pub(crate) const LOOKUPS_PER_JOB: usize = 10;

/// The largest text file `read_version` reads: each page streams the whole file from history
/// on the matter's thread, so a larger one is refused by its size before any read.
pub(crate) const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;

/// The largest Word file `read_version` reads, refused by its size before any copy, and the
/// copy's own cap: each read streams the file from history into a copy on the matter's thread,
/// up to [`MAX_DOCX_RUNS_PER_TURN`] a turn. Above [`MAX_TEXT_BYTES`], since a `.docx` is a zip
/// whose pictures can outweigh its text many times over; a size the matter's own cap could reach
/// would let one turn copy that much again and again.
pub(crate) const MAX_DOCX_BYTES: u64 = 25 * 1024 * 1024;

/// How long a call waits for the matter's thread, its own earlier job included, before it is
/// refused as unavailable.
pub(crate) const MATTER_WAIT: Duration = Duration::from_secs(60);

/// The matter a read gets, already open on its thread, with the size cap its settings hold.
pub(crate) struct Opened<'a> {
    /// The matter.
    pub(crate) matter: &'a Matter,
    /// `Limits::effective_cap`: a Word copy past it is refused, as the blackline's copies are.
    pub(crate) cap: u64,
}

/// A read run on a matter's thread (`MatterJob::Read`) with the matter when its thread holds it
/// open, and with none otherwise. Dropped unrun, its caller hears nothing and gives up.
pub(crate) struct MatterRead(Box<ReadFn>);

/// What a [`MatterRead`] runs.
type ReadFn = dyn FnOnce(Option<Opened<'_>>) + Send;

impl MatterRead {
    /// A read of `run` (tests of the matter's thread).
    #[cfg(test)]
    pub(crate) fn new(run: impl FnOnce(Option<Opened<'_>>) + Send + 'static) -> Self {
        Self(Box::new(run))
    }

    /// Runs it with `opened`.
    pub(crate) fn run(self, opened: Option<Opened<'_>>) {
        (self.0)(opened);
    }
}

impl std::fmt::Debug for MatterRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MatterRead")
    }
}

/// What the matter's log records of some versions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Logged {
    /// When each was recorded (its `CheckpointRecorded`), as Versions shows it.
    pub(crate) at: BTreeMap<CheckpointId, Timestamp>,
    /// For each restore, the version it restored from (its `VersionRestored`).
    pub(crate) restored_from: BTreeMap<CheckpointId, CheckpointId>,
}

/// The matter's thread could not take a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreachable;

/// How the backend reaches the matter: the host's is [`source::HostSource`]; tests hold a matter.
pub(crate) trait VersionsSource: Send + Sync {
    /// Queues `read` on `matter`'s thread, which runs it in turn with its other jobs, with the
    /// matter when it is already open and with none otherwise; with no thread it runs at once
    /// with none. [`Unreachable`] when it cannot be queued; `read` is then dropped unrun. A
    /// read dropped unrun later (the thread ended) is seen by the caller as no answer.
    fn on_matter(&self, matter: MatterId, read: MatterRead) -> Result<(), Unreachable>;

    /// What `matter`'s log records of `versions`; a version the log has no row of is left out.
    fn logged(&self, matter: MatterId, versions: &[CheckpointId]) -> Logged;
}

/// Whether this launch's agent got the tool, for the undo note's line that points at it. The
/// transport sets it at each launch; a firm's `managed-mcp.json` means it is [`Self::Absent`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VersionsTool {
    /// The agent has `clauseo-versions`.
    Offered,
    /// It does not.
    #[default]
    Absent,
}

/// What one session's backend needs.
pub(crate) struct VersionsConfig {
    /// The session: its matter is the only one the tools read.
    pub(crate) session: SessionKey,
    /// How the matter is reached.
    pub(crate) source: Arc<dyn VersionsSource>,
    /// The host's clock: the id of a Word file's private folder, and stetdoc's deadline.
    pub(crate) clock: Arc<dyn Clock>,
    /// The data folder: a Word file is copied under its `tmp/`.
    pub(crate) data_dir: PathBuf,
    /// The stetdoc program (`HostConfig::with_stetdoc`); without it a Word file is refused.
    pub(crate) stetdoc: Option<PathBuf>,
}

/// One session's tool: build it at launch, answer each message with [`Self::answer`].
#[derive(Clone)]
pub(crate) struct VersionsBackend {
    inner: Arc<Backend>,
}

struct Backend {
    config: VersionsConfig,
    deleted_readable: bool,
    caps: Mutex<caps::Caps>,
    docx: docx::Kept,
    /// The session's one job on the matter's thread.
    flight: Arc<flight::Flight>,
    /// Held while stetdoc runs: one run per session at a time.
    stetdoc_run: Mutex<()>,
    /// Word copies planned, each with its run reserved (tests).
    #[cfg(test)]
    copies_planned: std::sync::atomic::AtomicU32,
}

impl std::fmt::Debug for VersionsBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VersionsBackend")
            .field("session", &self.inner.config.session)
            .finish_non_exhaustive()
    }
}

impl VersionsBackend {
    /// The backend of `config.session`, with [`DELETED_FILES_READABLE`]: the only way to build
    /// one outside tests.
    pub(crate) fn new(config: VersionsConfig) -> Self {
        Self::build(config, DELETED_FILES_READABLE)
    }

    /// The backend with the deleted-file limit set to `deleted_readable` (tests of each value).
    #[cfg(test)]
    fn with_deleted_readable(config: VersionsConfig, deleted_readable: bool) -> Self {
        Self::build(config, deleted_readable)
    }

    fn build(config: VersionsConfig, deleted_readable: bool) -> Self {
        Self {
            inner: Arc::new(Backend {
                config,
                deleted_readable,
                caps: Mutex::new(caps::Caps::default()),
                docx: docx::Kept::default(),
                flight: Arc::default(),
                stetdoc_run: Mutex::new(()),
                #[cfg(test)]
                copies_planned: std::sync::atomic::AtomicU32::new(0),
            }),
        }
    }

    /// The answer to one JSON-RPC message from the agent, or `None` for a notification or a
    /// response. Blocks the calling thread while the matter's thread and stetdoc work: call it
    /// off the actor, one thread per message.
    pub(crate) fn answer(&self, message: &Value) -> Option<Value> {
        mcp::answer(&self.inner, message)
    }

    /// The handle the host thread moves when a turn starts and ends.
    pub(crate) fn turns(&self) -> VersionsTurns {
        VersionsTurns {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Where the session's turn is, for the call caps: written by the host thread.
#[derive(Clone)]
pub(crate) struct VersionsTurns {
    inner: Arc<Backend>,
}

impl std::fmt::Debug for VersionsTurns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("VersionsTurns")
            .field(&self.inner.config.session)
            .finish()
    }
}

impl VersionsTurns {
    /// A turn's prompt goes to the agent: its counts, and the between-turns ones, start at 0.
    pub(crate) fn start(&self) {
        lock(&self.inner.caps).start_turn();
    }

    /// The turn ended: calls now count against a fresh between-turns bucket.
    pub(crate) fn end(&self) {
        lock(&self.inner.caps).end_turn();
    }
}

/// `at` as an RFC 3339 time in UTC, to the second: `2026-11-16T10:05:07Z`.
fn utc_time(at: Timestamp) -> String {
    let secs = at.as_millis().div_euclid(1000);
    let (year, month, day) = crate::connectors::civil_from_days(secs.div_euclid(86_400));
    let of_day = secs.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
