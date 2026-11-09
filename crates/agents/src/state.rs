//! What `Agents` reports: one [`InstallState`] and one [`SigninState`] per agent, sent as
//! [`AgentsEvent`]s, and the [`Installed`] adapter R20 turns into the `agents.<id>` settings
//! entry.

use std::path::{Component, Path, PathBuf};

use clauseo_codex_appserver::codex_js_candidates;
use clauseo_protocol::AgentId;

use crate::log::InstallLog;
use crate::signin::{SigninNote, SigninState};

/// An event on the channel `Agents::start` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentsEvent {
    /// An agent's install state changed, or a download made progress.
    Install {
        /// The agent.
        agent: AgentId,
        /// Its new state.
        state: InstallState,
    },
    /// The Python runtime's state changed, or its download made progress. One runtime serves
    /// every agent; it is installed as the second step of an agent's install job, after the
    /// agent is `Ready`, and its failure never changes the agent's state.
    Python {
        /// Its new state.
        state: PythonState,
    },
    /// An agent's sign-in state changed.
    Signin {
        /// The agent.
        agent: AgentId,
        /// Its new state.
        state: SigninState,
    },
    /// Open `url` in the lawyer's browser: an address Claude's `BROWSER` shim captured whose host
    /// is one of the snapshot's sign-in hosts (Unix only). Sent once per address; an address read
    /// only from a run's output is never sent, and Codex never sends one: its device page is in
    /// [`SigninState::DeviceCode`], and its browser login opens the tab itself.
    OpenUrl {
        /// The agent signing in.
        agent: AgentId,
        /// The address to open.
        url: String,
    },
    /// A sign-in note for R20's log (`claude_reuse_fallback`, `codex_link_failed`).
    SigninNote {
        /// The agent.
        agent: AgentId,
        /// What happened.
        note: SigninNote,
    },
    /// An agent's sign-in job ended with no new state (a status check that could not tell, say):
    /// the last [`AgentsEvent::Signin`] state stands, and the agent has no job running.
    SigninUnchanged {
        /// The agent.
        agent: AgentId,
    },
}

/// Where an agent's install is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallState {
    /// Not installed, and no install running. `download_bytes` is what an install would fetch
    /// here, Node's archive included when Node would be downloaded (UD1's "About {size} MB").
    /// Python's archive is never in it: [`PythonState::Missing`] carries its size, once for the
    /// computer. The install's disk check still counts Python.
    Missing {
        /// Bytes an install would download.
        download_bytes: u64,
    },
    /// Waiting to try again by itself; no try is counted. `reason` is
    /// [`FailureKind::Offline`] today: name resolution or connect failed with no route.
    Waiting {
        /// Why it waits.
        reason: FailureKind,
        /// The log so far, for "Details".
        log: InstallLog,
    },
    /// Downloading. Bytes received and the total of every file this install fetches, Node's
    /// archive included and never named; at most one such event every 250 ms.
    Downloading {
        /// Bytes received so far, partial files kept from earlier tries included.
        received: u64,
        /// Bytes in all, from each file's `Content-Length` or its known size.
        total: u64,
    },
    /// Every file is here and checked; npm is installing from them.
    Installing,
    /// Installed: the receipt is written.
    Ready {
        /// The adapter's version, from the pins.
        version: String,
    },
    /// Stopped with a failure; `retry` starts again.
    Failed(Failure),
    /// Paused because another agent's install started; its own `install` resumes it, keeping
    /// its partial files.
    Paused,
    /// IT's runtime: policy sets `agents.<id>`, so nothing is installed for it.
    Managed,
}

/// Where the Python runtime is: one for the computer, kept apart from each agent's state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PythonState {
    /// This build has no Python for this platform (Linux, or before Clauseo publishes the
    /// archives): nothing is fetched.
    Unavailable,
    /// Not installed, and no install of it running. `download_bytes` is its archive's size, in
    /// no agent's `InstallState::Missing`: a window adds it once to the missing agents' sizes.
    Missing {
        /// Bytes its install would download.
        download_bytes: u64,
    },
    /// Offline: waiting to try again by itself, as an agent's install does.
    Waiting {
        /// Why it waits: [`FailureKind::Offline`].
        reason: FailureKind,
        /// The log so far.
        log: InstallLog,
    },
    /// Downloading its archive (or copying it from the offline bundle).
    Downloading {
        /// Bytes received so far, a partial file kept from an earlier try included.
        received: u64,
        /// The archive's size.
        total: u64,
    },
    /// The archive is here and checked; it is being unpacked and run once.
    Installing,
    /// Installed: its receipt is written.
    Ready {
        /// Python's version (`3.13.18`).
        version: String,
    },
    /// Stopped with a failure; a retry of any agent's install tries again.
    Failed(PythonFailure),
}

/// A stopped install of the Python runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PythonFailure {
    /// Which kind.
    pub kind: PythonFailureKind,
    /// For [`PythonFailureKind::RunBlocked`]: which kind of policy the refusal's error code
    /// names; `None` for every other kind.
    pub blocked_by: Option<PythonBlocker>,
    /// The log, for "Details".
    pub log: InstallLog,
}

/// What refused to start the Python runtime, read only from the failed start's error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PythonBlocker {
    /// App Control for Business (WDAC) or Smart App Control: Windows error 4551, or its NT
    /// status `0xC0E90002`.
    AppControl,
    /// `AppLocker` or Software Restriction Policies: Windows error 1260.
    AppLocker,
    /// Any other refusal: access denied, or `STATUS_INVALID_IMAGE_HASH`.
    Unknown,
}

impl PythonBlocker {
    /// The word the log's `blocked by:` line uses.
    pub fn word(self) -> &'static str {
        match self {
            PythonBlocker::AppControl => "app_control",
            PythonBlocker::AppLocker => "applocker",
            PythonBlocker::Unknown => "unknown",
        }
    }
}

/// Why the Python runtime's install stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PythonFailureKind {
    /// A proxy or TLS refused the download, as [`FailureKind::Blocked`].
    Blocked,
    /// Too little free space, or `ENOSPC` while writing.
    DiskFull,
    /// The download, the offline bundle's copy or the unpack failed, after three tries.
    Failed,
    /// The OS refused to start the unpacked interpreter or load its libraries (access denied,
    /// or a policy such as Smart App Control, `AppLocker` or WDAC).
    RunBlocked,
    /// The unpacked interpreter started but its check failed: a non-zero exit, no answer, or
    /// no answer within 20 s.
    RunFailed,
}

impl PythonFailureKind {
    /// The word the log's `result:` line uses.
    pub fn word(self) -> &'static str {
        match self {
            PythonFailureKind::Blocked => "blocked",
            PythonFailureKind::DiskFull => "disk_full",
            PythonFailureKind::Failed => "failed",
            PythonFailureKind::RunBlocked => "run_blocked",
            PythonFailureKind::RunFailed => "run_failed",
        }
    }
}

/// Why an install stopped. Each kind is one error code, which R20 adds to `ErrorCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FailureKind {
    /// Name resolution or connect failed and the OS reports no route. Never a failure state:
    /// the install waits and retries every 30 s by itself.
    Offline,
    /// A 403 or 407 from a proxy, a TLS failure, or an HTML page where a file should be.
    Blocked,
    /// Too little free space for the snapshot's need, or `ENOSPC` while writing.
    DiskFull,
    /// Anything else, after three tries.
    Failed,
    /// The files all downloaded and checked, but setting them up failed, after three tries:
    /// unpacking Node, `npm ci`, the check for links, the rename into place or the receipt.
    /// Often an antivirus holding the fresh files on Windows.
    SetupFailed,
}

impl FailureKind {
    /// The error code: `agent.install_offline`, `_blocked`, `_disk_full`, `_failed` or
    /// `_setup_failed`.
    pub fn code(self) -> &'static str {
        match self {
            FailureKind::Offline => "agent.install_offline",
            FailureKind::Blocked => "agent.install_blocked",
            FailureKind::DiskFull => "agent.install_disk_full",
            FailureKind::Failed => "agent.install_failed",
            FailureKind::SetupFailed => "agent.install_setup_failed",
        }
    }

    /// The word the log's `result:` line uses.
    pub fn word(self) -> &'static str {
        match self {
            FailureKind::Offline => "offline",
            FailureKind::Blocked => "blocked",
            FailureKind::DiskFull => "disk_full",
            FailureKind::Failed => "failed",
            FailureKind::SetupFailed => "setup_failed",
        }
    }
}

/// A stopped install.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Failure {
    /// Which kind.
    pub kind: FailureKind,
    /// For [`FailureKind::DiskFull`]: the space needed, in MB (10^6 bytes).
    pub need_mb: Option<u64>,
    /// For [`FailureKind::DiskFull`]: the space free, in MB.
    pub free_mb: Option<u64>,
    /// The log, for "Details".
    pub log: InstallLog,
}

/// A finished install: R20 writes `command: node`, `args: [script]` as the `agents.<id>`
/// settings entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Installed {
    /// The Node to run the adapter with: the user's (22 or later) or the downloaded one.
    pub node: PathBuf,
    /// The adapter's script, `<folder>/node_modules/<root package>/<bin>`.
    pub script: PathBuf,
    /// The install folder, `<data_dir>/agents/<id>/install/<hash>`.
    pub folder: PathBuf,
    /// The adapter's version.
    pub version: String,
}

impl Installed {
    /// The Codex CLI's `codex.js` in this install, which every Codex sign-in, status, logout and
    /// `app-server` run uses: `script` is codex-acp's ACP adapter, which ignores those commands
    /// and exits 0. The candidates are `codex_appserver`'s [`codex_js_candidates`], the layout
    /// `limits`' reader follows too (the adapter package's own `node_modules`, then the install
    /// folder's), always inside `folder`, never beside Node, which may be the user's own. The
    /// first that is a regular file reached through no link below `folder` is it. `None` when
    /// none is: the job then fails rather than run the adapter.
    pub(crate) fn codex_cli(&self) -> Option<PathBuf> {
        codex_js_candidates(&self.folder, &self.script)
            .into_iter()
            .find(|candidate| regular_file_below(&self.folder, candidate))
    }
}

/// Whether `file` is a regular file inside `folder` with no link from `folder` down to it.
fn regular_file_below(folder: &Path, file: &Path) -> bool {
    let Ok(rest) = file.strip_prefix(folder) else {
        return false;
    };
    let mut path = folder.to_path_buf();
    let mut parts = rest.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(part) = part else {
            return false;
        };
        path.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        let fits = if parts.peek().is_some() {
            meta.is_dir()
        } else {
            meta.is_file()
        };
        if !fits {
            return false;
        }
    }
    true
}
