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
}

/// Where an agent's install is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallState {
    /// Not installed, and no install running. `download_bytes` is what an install would fetch
    /// here, Node's archive included when Node would be downloaded (UD1's "About {size} MB").
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
}

impl FailureKind {
    /// The error code: `agent.install_offline`, `_blocked`, `_disk_full` or `_failed`.
    pub fn code(self) -> &'static str {
        match self {
            FailureKind::Offline => "agent.install_offline",
            FailureKind::Blocked => "agent.install_blocked",
            FailureKind::DiskFull => "agent.install_disk_full",
            FailureKind::Failed => "agent.install_failed",
        }
    }

    /// The word the log's `result:` line uses.
    pub fn word(self) -> &'static str {
        match self {
            FailureKind::Offline => "offline",
            FailureKind::Blocked => "blocked",
            FailureKind::DiskFull => "disk_full",
            FailureKind::Failed => "failed",
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
