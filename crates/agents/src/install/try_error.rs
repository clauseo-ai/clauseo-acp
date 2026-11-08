//! How a try ended badly ([`TryError`]) and what the job does next ([`Next`]).

use std::io;

use crate::config::AgentsConfig;
use crate::download::{FetchError, FetchKind, write_kind};
use crate::log::redact;
use crate::state::FailureKind;
use crate::write::ExtractError;

/// How a try ended badly, and what the job does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Next {
    /// Wait 30 s and try again, no try counted.
    Offline,
    /// Stop now as `Blocked`.
    Blocked,
    /// Stop now as `DiskFull`.
    DiskFull,
    /// Count a try; the third is `Failed`.
    Retry,
    /// Stop now as `Failed`: a retry cannot help.
    Stop,
    /// Paused: end quietly.
    Cancelled,
}

/// A try that did not finish: what comes next, and the log's lines for it.
#[derive(Debug)]
pub(crate) struct TryError {
    pub(super) next: Next,
    pub(super) host: Option<String>,
    pub(super) status: Option<u16>,
    /// Redacted.
    pub(super) detail: String,
    /// It came after every file was fetched and checked: in setting them up (`npm`).
    setup: bool,
}

impl TryError {
    /// A failure no retry can help: the job stops now as `Failed`, with `detail` redacted.
    pub(crate) fn stop(detail: &str, config: &AgentsConfig) -> TryError {
        TryError::new(Next::Stop, detail, config)
    }

    /// A try to count, with `detail` redacted.
    pub(crate) fn retry(detail: &str, config: &AgentsConfig) -> TryError {
        TryError::new(Next::Retry, detail, config)
    }

    pub(super) fn new(next: Next, detail: &str, config: &AgentsConfig) -> TryError {
        TryError {
            next,
            host: None,
            status: None,
            detail: redact(detail, &config.data_dir),
            setup: false,
        }
    }

    pub(crate) fn cancelled() -> TryError {
        TryError {
            next: Next::Cancelled,
            host: None,
            status: None,
            detail: String::new(),
            setup: false,
        }
    }

    /// A local file error: a full disk, else a try to count.
    pub(crate) fn io(what: &str, err: &io::Error, config: &AgentsConfig) -> TryError {
        let next = match write_kind(err) {
            FetchKind::DiskFull => Next::DiskFull,
            _ => Next::Retry,
        };
        TryError::new(next, &format!("{what}: {err}"), config)
    }

    pub(crate) fn extract(err: &ExtractError, config: &AgentsConfig) -> TryError {
        match err {
            ExtractError::Io(err) => TryError::io("unpacking Node failed", err, config),
            other => TryError::retry(&other.to_string(), config),
        }
    }

    /// This error, marked as one in setting up the fetched files.
    pub(super) fn in_setup(mut self) -> TryError {
        self.setup = true;
        self
    }

    /// The kind a job that stops on this error fails with, for `Next::Retry` and `Next::Stop`:
    /// `SetupFailed` once every file was fetched and checked, else `Failed`.
    pub(super) fn failed_kind(&self) -> FailureKind {
        if self.setup {
            FailureKind::SetupFailed
        } else {
            FailureKind::Failed
        }
    }

    /// npm's `ENOSPC` is a full disk; any other npm failure is a try to count.
    pub(crate) fn npm(detail: &str, enospc: bool, config: &AgentsConfig) -> TryError {
        let next = if enospc { Next::DiskFull } else { Next::Retry };
        TryError::new(next, detail, config)
    }
}

impl From<FetchError> for TryError {
    fn from(err: FetchError) -> TryError {
        let next = match err.kind {
            FetchKind::Offline => Next::Offline,
            FetchKind::Blocked => Next::Blocked,
            FetchKind::DiskFull => Next::DiskFull,
            FetchKind::Cancelled => Next::Cancelled,
            FetchKind::Integrity | FetchKind::Failed => Next::Retry,
        };
        TryError {
            next,
            host: err.host,
            status: err.status,
            detail: err.detail,
            setup: false,
        }
    }
}
