//! A try's failure: what the job does next ([`Next`]), the step that failed ([`Step`]) and its
//! cause in fixed words, the download log's lines for it, and the app log's failed-try line.

use std::io;

use clauseo_protocol::AgentId;

use super::Run;
use crate::config::AgentsConfig;
use crate::download::{FetchError, FetchKind, write_kind};
use crate::log::{InstallLog, redact};
use crate::write::ExtractError;

/// How a try ended badly, and what the job does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
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

impl Next {
    /// The fixed words for the failed-try line: whether the job tries again.
    fn verdict(self) -> &'static str {
        match self {
            Next::Offline => "offline, waiting to try again",
            Next::Retry => "transient, retrying",
            Next::Blocked | Next::DiskFull | Next::Stop => "permanent, stopping",
            Next::Cancelled => "paused",
        }
    }
}

/// The step of a try that failed, in fixed words for the failed-try line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Before any file: the paths, the lock, Node's choice, the space check.
    Prepare,
    /// Fetching a file, or copying it from the bundle.
    Fetch,
    /// A fetched file's digest did not match.
    Verify,
    /// Unpacking the downloaded Node.
    UnpackNode,
    /// `npm ci`, and the link check after it.
    NpmInstall,
    /// The rename of the temp folder to `<hash>`.
    Rename,
    /// Writing the receipt.
    Receipt,
}

impl Step {
    fn word(self) -> &'static str {
        match self {
            Step::Prepare => "prepare",
            Step::Fetch => "fetch",
            Step::Verify => "verify",
            Step::UnpackNode => "unpack node",
            Step::NpmInstall => "npm install",
            Step::Rename => "rename",
            Step::Receipt => "receipt",
        }
    }
}

/// A try that did not finish: what comes next, and the log's lines for it.
#[derive(Debug)]
pub(crate) struct TryError {
    pub(super) next: Next,
    host: Option<String>,
    status: Option<u16>,
    /// Redacted.
    detail: String,
    /// The step that failed, once known.
    step: Option<Step>,
    /// npm's exit code, when npm ran and failed.
    npm_exit: Option<i32>,
    /// The cause in fixed words ([`npm_cause`], an io error's kind), when known.
    cause: Option<&'static str>,
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
            step: None,
            npm_exit: None,
            cause: None,
        }
    }

    pub(crate) fn cancelled() -> TryError {
        TryError {
            next: Next::Cancelled,
            host: None,
            status: None,
            detail: String::new(),
            step: None,
            npm_exit: None,
            cause: None,
        }
    }

    /// The error with its step set, unless a deeper one set it already.
    pub(crate) fn at(mut self, step: Step) -> TryError {
        self.step.get_or_insert(step);
        self
    }

    /// The error with its cause set, unless one is set already.
    pub(crate) fn because(mut self, cause: &'static str) -> TryError {
        self.cause.get_or_insert(cause);
        self
    }

    /// A local file error: a full disk, else a try to count.
    pub(crate) fn io(what: &str, err: &io::Error, config: &AgentsConfig) -> TryError {
        let next = match write_kind(err) {
            FetchKind::DiskFull => Next::DiskFull,
            _ => Next::Retry,
        };
        TryError::new(next, &format!("{what}: {err}"), config).because(io_cause(err))
    }

    pub(crate) fn extract(err: &ExtractError, config: &AgentsConfig) -> TryError {
        match err {
            ExtractError::Io(err) => TryError::io("unpacking Node failed", err, config),
            other => TryError::retry(&other.to_string(), config),
        }
    }

    /// npm's `ENOSPC` is a full disk; any other npm failure is a try to count. `exit` is npm's
    /// exit code (`None` for a signal), `code_line` its `npm error code X` line.
    pub(crate) fn npm(
        detail: &str,
        exit: Option<i32>,
        code_line: Option<&str>,
        module_missing: bool,
        config: &AgentsConfig,
    ) -> TryError {
        let enospc = code_line.is_some_and(|line| line.ends_with("ENOSPC"));
        let next = if enospc { Next::DiskFull } else { Next::Retry };
        let mut err = TryError::new(next, detail, config)
            .at(Step::NpmInstall)
            .because(npm_cause(code_line, module_missing));
        err.npm_exit = exit;
        err
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
        let (step, cause) = match err.kind {
            FetchKind::Integrity => (Step::Verify, "checksum mismatch"),
            FetchKind::Offline => (Step::Fetch, "network unreachable"),
            FetchKind::Blocked => (Step::Fetch, "blocked by a proxy or firewall"),
            FetchKind::DiskFull => (Step::Fetch, "disk full"),
            FetchKind::Cancelled => (Step::Fetch, "paused"),
            FetchKind::Failed => (Step::Fetch, "download failed"),
        };
        TryError {
            next,
            host: err.host,
            status: err.status,
            detail: err.detail,
            step: Some(step),
            npm_exit: None,
            cause: Some(cause),
        }
    }
}

/// npm's failure in fixed words, from its `npm error code X` line: a file locked or refused
/// (`EBUSY`, `EPERM`, `EACCES`: on Windows most likely an antivirus scanning the files npm
/// has just written), a file missing, a full disk, a tarball changed after its check, or npm
/// unable to load its own modules.
pub(crate) fn npm_cause(code_line: Option<&str>, module_missing: bool) -> &'static str {
    let code = code_line.and_then(|line| line.strip_prefix("npm error code "));
    match code {
        Some("EBUSY" | "EPERM" | "EACCES") => {
            "file in use or refused (EBUSY, EPERM or EACCES; likely an antivirus lock)"
        }
        Some("ENOENT") => "file missing (ENOENT)",
        Some("ENOSPC") => "disk full (ENOSPC)",
        Some("EINTEGRITY") => "tarball changed after its check (EINTEGRITY)",
        Some(_) => "other npm error code",
        None if module_missing => "npm could not load its own modules",
        None => "no npm error code",
    }
}

/// A local file error's cause in fixed words.
fn io_cause(err: &io::Error) -> &'static str {
    match err.kind() {
        io::ErrorKind::PermissionDenied => "permission denied or file in use (likely antivirus)",
        io::ErrorKind::ResourceBusy => "file in use (likely an antivirus lock)",
        io::ErrorKind::NotFound => "file missing",
        io::ErrorKind::StorageFull => "disk full",
        _ => "file error",
    }
}

/// The app log's line for a failed try, in fixed words: the step, npm's exit code, the cause and
/// whether the job tries again. No detail and no path: the download log holds those.
pub(crate) fn log_failed_try(agent: &AgentId, run: &Run, err: &TryError) {
    if err.next == Next::Cancelled {
        return;
    }
    let step = err.step.map_or("unknown", Step::word);
    let cause = err.cause.unwrap_or("unknown");
    let npm_exit = err
        .npm_exit
        .map_or_else(|| "none".to_owned(), |code| code.to_string());
    tracing::warn!(
        %agent,
        attempt = run.attempts,
        step,
        npm_exit = %npm_exit,
        cause,
        status = ?err.status,
        "install: try failed at {step}: {cause}; {}",
        err.next.verdict()
    );
}

/// Adds a failed try's host, status and detail to `log`.
pub(crate) fn push_error(log: &mut InstallLog, err: &TryError) {
    if let Some(host) = &err.host {
        log.push(format!("host: {host}"));
    }
    if let Some(status) = err.status {
        log.push(format!("status: {status}"));
    }
    if !err.detail.is_empty() {
        log.push(err.detail.clone());
    }
}
