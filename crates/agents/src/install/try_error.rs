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
    /// Whether it is a set-up failure: `None` until [`TryError::in_setup`] marks it, and
    /// `Some(false)` for one that is not, whatever step it came from ([`TryError::not_setup`]).
    setup: Option<bool>,
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
            setup: None,
        }
    }

    pub(crate) fn cancelled() -> TryError {
        TryError {
            next: Next::Cancelled,
            host: None,
            status: None,
            detail: String::new(),
            setup: None,
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

    /// This error, marked as one in setting up the fetched files: unless it is a `Stop` (a
    /// deterministic failure, which a retry cannot help, so not `SetupFailed`'s "try again")
    /// or already marked [`TryError::not_setup`].
    pub(super) fn in_setup(mut self) -> TryError {
        if self.next != Next::Stop {
            self.setup.get_or_insert(true);
        }
        self
    }

    /// This error, marked as no set-up failure even when it comes from the set-up step: the
    /// snapshot not knowing the agent, Node's archive missing, npm's `EINTEGRITY` (a tarball
    /// that no longer matches its checked hash, a download's failure). It fails as `Failed`.
    pub(super) fn not_setup(mut self) -> TryError {
        self.setup = Some(false);
        self
    }

    /// The kind a job that stops on this error fails with, for `Next::Retry` and `Next::Stop`:
    /// `SetupFailed` once every file was fetched and checked, else `Failed`.
    pub(super) fn failed_kind(&self) -> FailureKind {
        if self.setup == Some(true) {
            FailureKind::SetupFailed
        } else {
            FailureKind::Failed
        }
    }

    /// npm's failure with its error `code`, if it printed one: `ENOSPC` is a full disk; any
    /// other is a try to count, `EINTEGRITY` as no set-up failure ([`TryError::not_setup`]).
    pub(crate) fn npm(detail: &str, code: Option<&str>, config: &AgentsConfig) -> TryError {
        match code {
            Some("ENOSPC") => TryError::new(Next::DiskFull, detail, config),
            Some("EINTEGRITY") => TryError::retry(detail, config).not_setup(),
            _ => TryError::retry(detail, config),
        }
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
            setup: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::TryError;
    use crate::config::AgentsConfig;
    use crate::platform::{Arch, Os, Platform};
    use crate::state::FailureKind;

    fn config() -> AgentsConfig {
        AgentsConfig::new(PathBuf::from("/data"), Platform::new(Os::Linux, Arch::X64))
    }

    #[test]
    fn a_retry_in_the_set_up_is_a_set_up_failure() {
        let err = TryError::retry("npm ci exited with code 1", &config()).in_setup();
        assert_eq!(err.failed_kind(), FailureKind::SetupFailed);
    }

    #[test]
    fn a_stop_in_the_set_up_keeps_the_failed_kind() {
        let err = TryError::stop("a package has no tarball", &config()).in_setup();
        assert_eq!(err.failed_kind(), FailureKind::Failed);
    }

    #[test]
    fn npm_s_eintegrity_keeps_the_failed_kind_and_enospc_is_a_full_disk() {
        let config = config();
        let err = TryError::npm("npm ci exited", Some("EINTEGRITY"), &config).in_setup();
        assert_eq!(err.failed_kind(), FailureKind::Failed);
        let err = TryError::npm("npm ci exited", Some("EBUSY"), &config).in_setup();
        assert_eq!(err.failed_kind(), FailureKind::SetupFailed);
        let err = TryError::npm("npm ci exited", Some("ENOSPC"), &config);
        assert_eq!(err.next, super::Next::DiskFull);
    }
}
