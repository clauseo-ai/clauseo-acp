//! [`InstallProgress`], an agent's install state on the wire, and [`InstallLog`], the bounded
//! log a waiting or failed install carries for "Details".

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{Count, ErrorCode};

/// The most lines an [`InstallLog`] holds, its two heading lines and the note of lines left
/// out included.
pub const MAX_INSTALL_LOG_LINES: usize = 64;

/// The most UTF-8 bytes in one line of an [`InstallLog`].
pub const MAX_INSTALL_LOG_LINE_BYTES: usize = 400;

/// Where an agent's install is: `agents`' `InstallState`. There is no end estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum InstallProgress {
    /// Not installed, and no install running.
    Missing {
        /// Bytes an install would download here, Node's archive included when Node would be
        /// downloaded ("About {size} MB"). Python's archive is never in it: it is in
        /// [`PythonProgress::Missing`], once for the computer, so a window adds that once to
        /// the missing agents' sizes.
        download_bytes: Count,
    },
    /// Offline: the install waits and tries again by itself.
    Waiting {
        /// Why it waits: `agent.install_offline`.
        code: ErrorCode,
        /// The hosts the install contacts, for "Ask your IT team".
        hosts: Vec<String>,
        /// The log so far, for "Details".
        log: InstallLog,
    },
    /// Downloading.
    Downloading {
        /// Bytes received so far.
        received: Count,
        /// Bytes in all, Node's archive included.
        total: Count,
    },
    /// Every file is here and checked; npm is installing from them.
    Installing,
    /// Installed.
    Ready {
        /// The adapter's version.
        version: String,
    },
    /// Stopped with a failure; [`crate::SetupCommand::RetryAgentInstall`] starts again.
    Failed {
        /// Why: `agent.install_blocked`, `agent.install_disk_full`, `agent.install_failed` or
        /// `agent.install_setup_failed`.
        code: ErrorCode,
        /// For `agent.install_disk_full`: the space needed, in MB (10^6 bytes).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        need_mb: Option<Count>,
        /// For `agent.install_disk_full`: the space free, in MB.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        free_mb: Option<Count>,
        /// The hosts the install contacts, for "Ask your IT team".
        hosts: Vec<String>,
        /// The log, for "Details".
        log: InstallLog,
    },
    /// Paused because another agent's install started; `InstallAgent` resumes it.
    Paused,
    /// The firm's policy runs this agent, so Clauseo installs nothing for it.
    Managed,
}

/// Where the Python runtime the agents use is: `agents`' `PythonState`. One for the computer,
/// installed after an agent's install in the same job; its failure never changes an agent's
/// [`InstallProgress`]. Rides on [`crate::SetupEvent::SetupInfo`]'s optional `python`.
///
/// Its retry: [`crate::SetupCommand::RetryAgentInstall`] for **any** ready agent retries
/// Python (one for the computer); the window may send any ready plan's id. That agent's own
/// install is left as it is: it stays `Ready`, and nothing of it is fetched or installed again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PythonProgress {
    /// This build has no Python for this computer: nothing is fetched.
    Unavailable,
    /// Not installed, and no install of it running.
    Missing {
        /// Bytes its install would download. No agent's [`InstallProgress::Missing`] counts
        /// them, so the size to show is the missing agents' sum plus this, once.
        download_bytes: Count,
    },
    /// Offline: its install waits and tries again by itself.
    Waiting {
        /// Why it waits, as an agent's `Waiting`: `agent.install_offline`. Missing from an
        /// older host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<ErrorCode>,
        /// The hosts Python's download contacts, for "Ask your IT team"; empty (left out) from
        /// an older host.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hosts: Vec<String>,
        /// The log so far, for "Details".
        log: InstallLog,
    },
    /// Downloading.
    Downloading {
        /// Bytes received so far.
        received: Count,
        /// Bytes in all.
        total: Count,
    },
    /// Unpacking and checking that it runs.
    Installing,
    /// Installed.
    Ready {
        /// Python's version.
        version: String,
    },
    /// Stopped with a failure; [`crate::SetupCommand::RetryAgentInstall`] for any ready agent
    /// tries again.
    Failed {
        /// Why.
        cause: PythonFailureCause,
        /// For a download-side cause, the agent install's code for it: `agent.install_blocked`,
        /// `agent.install_disk_full` or `agent.install_failed`. Missing for `run_blocked` and
        /// `run_failed` (no code names them), and from an older host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<ErrorCode>,
        /// For [`PythonFailureCause::Blocked`]: the hosts Python's download contacts, for "Ask
        /// your IT team"; empty (left out) otherwise.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hosts: Vec<String>,
        /// For [`PythonFailureCause::RunBlocked`]: which kind of policy refused the start, read
        /// from the error code alone. Missing for every other cause, and from an older host.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_by: Option<PythonBlockedBy>,
        /// The log, for "Details".
        log: InstallLog,
    },
}

/// What refused to start Python, for [`PythonFailureCause::RunBlocked`]: read only from the
/// error code the failed start returned, with no further probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PythonBlockedBy {
    /// App Control for Business (WDAC) or Smart App Control: Windows error 4551
    /// (`ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION`) or its NT status `0xC0E90002`.
    AppControl,
    /// `AppLocker` or Software Restriction Policies: Windows error 1260
    /// (`ERROR_ACCESS_DISABLED_BY_POLICY`).
    #[serde(rename = "applocker")]
    AppLocker,
    /// Any other refusal: access denied, or `STATUS_INVALID_IMAGE_HASH` (`0xC0000428`).
    Unknown,
}

/// Why the Python runtime's install stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PythonFailureCause {
    /// A proxy or TLS refused the download.
    Blocked,
    /// Too little free space.
    DiskFull,
    /// The download, the offline bundle's copy or the unpack failed after its tries.
    Failed,
    /// This computer refused to start Python or load its libraries (a policy, or access
    /// denied).
    RunBlocked,
    /// Python started but its check failed or did not answer.
    RunFailed,
}

/// An install's log as "Details" shows it: English lines, already free of user names,
/// passwords and paths outside the data folder (`agents` redacts them). At most
/// [`MAX_INSTALL_LOG_LINES`] lines of at most [`MAX_INSTALL_LOG_LINE_BYTES`] bytes each; decoding
/// refuses more. JSON: `["Clauseo download log", "agent: claude-acp", ...]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct InstallLog(Vec<String>);

impl InstallLog {
    /// The log of `lines`, cut to fit: each line cut to [`MAX_INSTALL_LOG_LINE_BYTES`] at a
    /// character boundary, and when there are too many, the first two (the heading) and the
    /// last ones kept, with a line `… {n} lines left out` between them.
    pub fn new(lines: impl IntoIterator<Item = String>) -> Self {
        let mut lines: Vec<String> = lines.into_iter().map(cut_line).collect();
        if lines.len() > MAX_INSTALL_LOG_LINES {
            let head = 2;
            let tail = MAX_INSTALL_LOG_LINES - head - 1;
            let dropped = lines.len() - head - tail;
            let rest = lines.split_off(head);
            lines.push(format!("… {dropped} lines left out"));
            lines.extend(rest.into_iter().skip(dropped));
        }
        Self(lines)
    }

    /// The lines, in order.
    pub fn lines(&self) -> &[String] {
        &self.0
    }
}

/// `line` cut to [`MAX_INSTALL_LOG_LINE_BYTES`] at a character boundary.
pub(super) fn cut_line(mut line: String) -> String {
    if line.len() > MAX_INSTALL_LOG_LINE_BYTES {
        let end = (0..=MAX_INSTALL_LOG_LINE_BYTES)
            .rev()
            .find(|&at| line.is_char_boundary(at))
            .unwrap_or(0);
        line.truncate(end);
    }
    line
}

impl<'de> Deserialize<'de> for InstallLog {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let lines = Vec::<String>::deserialize(deserializer)?;
        if lines.len() > MAX_INSTALL_LOG_LINES {
            return Err(D::Error::custom(format_args!(
                "an install log holds at most {MAX_INSTALL_LOG_LINES} lines"
            )));
        }
        if lines
            .iter()
            .any(|line| line.len() > MAX_INSTALL_LOG_LINE_BYTES)
        {
            return Err(D::Error::custom(format_args!(
                "an install log line holds at most {MAX_INSTALL_LOG_LINE_BYTES} bytes"
            )));
        }
        Ok(Self(lines))
    }
}
