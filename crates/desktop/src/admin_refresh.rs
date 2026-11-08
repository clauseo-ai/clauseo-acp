//! The Safe workspace's admin copy, refreshed after an update (S15 tier B, ADR 0049 "Admin copy
//! refresh"). At start-up on Windows one `admin-refresh` thread compares the record's
//! `AdminCopySha256` with the SHA-256 of the install folder's `clauseo-sandbox-setup.exe`
//! (`clauseo_sandbox::windows::refresh::installed`); when they differ, it asks the SYSTEM task
//! `\Clauseo\SandboxRefresh` to run (`request_admin_refresh`), which blocks on COM, so it never
//! runs on the main thread or an async worker. Nothing waits for the thread or joins it: the
//! window, the host and the welcome screen never wait on it. It shows nothing; each outcome is
//! one line in host.log (target [`TARGET`]). Before the compare it logs the task's last run
//! (`last_refresh_run`), so a refresh that keeps failing shows in host.log.

/// The log target: host.log keeps `clauseo_*::lifecycle` INFO lines.
#[cfg(windows)]
const TARGET: &str = "clauseo_desktop::sandbox::lifecycle";

/// What the start-up compare decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshDecision {
    /// The record holds no admin copy hash: tier B isn't set up.
    NotSetUp,
    /// The install folder's set-up program is missing or unreadable.
    NoInstalledCopy,
    /// The admin copy is the installed set-up program.
    UpToDate,
    /// The installed set-up program changed: ask the task to refresh.
    Refresh,
}

/// Whether to ask for a refresh, from the record's hash and the installed copy's. Hashes are
/// compared as hex, case-insensitively; a recorded value that isn't 64 hex digits never matches.
pub(crate) fn refresh_needed(recorded: Option<&str>, installed: Option<&str>) -> RefreshDecision {
    let Some(recorded) = recorded else {
        return RefreshDecision::NotSetUp;
    };
    let Some(installed) = installed else {
        return RefreshDecision::NoInstalledCopy;
    };
    let hex = |text: &str| text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit());
    if hex(recorded) && hex(installed) && recorded.eq_ignore_ascii_case(installed) {
        RefreshDecision::UpToDate
    } else {
        RefreshDecision::Refresh
    }
}

/// A hash's first 8 characters, for the log.
#[cfg(windows)]
fn short(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}

/// Starts the `admin-refresh` thread, not joined. A failed spawn is logged and nothing else.
#[cfg(windows)]
pub(crate) fn spawn() {
    let spawned = std::thread::Builder::new()
        .name("admin-refresh".to_owned())
        .spawn(run);
    if let Err(err) = spawned {
        tracing::warn!(target: TARGET, %err, "admin refresh: no thread, nothing checked");
    }
}

/// One line for the refresh task's last run: its label, code and local time, at info, or at
/// warn for a failure or an unknown code (`last_run::last_run_label`). A failed read is one
/// debug line; nothing else depends on it.
#[cfg(windows)]
fn log_last_run() {
    use clauseo_sandbox::windows::refresh::last_refresh_run;
    use clauseo_sandbox::windows::refresh::last_run::{
        LastRun, LastRunLevel, last_run_label, ole_date_text,
    };
    let run = match last_refresh_run() {
        Ok(run) => run,
        Err(err) => {
            tracing::debug!(target: TARGET, %err, "admin refresh: the last run couldn't be read");
            return;
        }
    };
    let (label, level) = last_run_label(run);
    let (code, at) = match run {
        LastRun::NeverRun => (None, None),
        LastRun::Ran { code, at } => (Some(code), ole_date_text(at)),
    };
    let at = at.unwrap_or_default();
    match level {
        LastRunLevel::Info => {
            tracing::info!(target: TARGET, label, ?code, at, "admin refresh: last run");
        }
        LastRunLevel::Warn => {
            tracing::warn!(target: TARGET, label, ?code, at, "admin refresh: last run");
        }
    }
}

/// The thread's work: log the last run, read, compare, and ask the task when the copy changed.
#[cfg(windows)]
fn run() {
    use clauseo_sandbox::windows::refresh::{
        TaskError, installed_setup_sha256, recorded_admin_copy_sha256, request_admin_refresh,
    };
    log_last_run();
    let recorded = match recorded_admin_copy_sha256() {
        Ok(recorded) => recorded,
        Err(err) => {
            tracing::warn!(target: TARGET, %err, "admin refresh: the record couldn't be read");
            return;
        }
    };
    let installed = match recorded.as_ref().map(|_| installed_setup_sha256()) {
        None | Some(Ok(None)) => None,
        Some(Ok(Some(installed))) => Some(installed),
        Some(Err(err)) => {
            tracing::info!(target: TARGET, %err, "admin refresh: the installed set-up program couldn't be read");
            None
        }
    };
    let (recorded, installed) = (recorded.as_deref(), installed.as_deref());
    match refresh_needed(recorded, installed) {
        RefreshDecision::NotSetUp => {
            tracing::info!(target: TARGET, "admin refresh: no admin copy recorded, nothing to refresh");
        }
        RefreshDecision::NoInstalledCopy => {
            tracing::info!(target: TARGET, "admin refresh: no installed set-up program, nothing to compare");
        }
        RefreshDecision::UpToDate => {
            tracing::debug!(target: TARGET, "admin refresh: the admin copy is up to date");
        }
        RefreshDecision::Refresh => {
            let recorded = recorded.map(short).unwrap_or_default();
            let installed = installed.map(short).unwrap_or_default();
            match request_admin_refresh() {
                Ok(()) => {
                    tracing::info!(target: TARGET, recorded, installed, "admin refresh requested");
                }
                Err(TaskError::NotRegistered) => {
                    tracing::info!(target: TARGET, "admin refresh: the refresh task isn't registered, nothing to refresh");
                }
                Err(err) => {
                    tracing::warn!(target: TARGET, %err, "admin refresh: the task couldn't be started");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RefreshDecision, refresh_needed};

    const A: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    #[test]
    fn none_recorded_is_not_set_up() {
        assert_eq!(refresh_needed(None, Some(A)), RefreshDecision::NotSetUp);
        assert_eq!(refresh_needed(None, None), RefreshDecision::NotSetUp);
    }

    #[test]
    fn installed_file_missing_compares_nothing() {
        assert_eq!(
            refresh_needed(Some(A), None),
            RefreshDecision::NoInstalledCopy
        );
    }

    #[test]
    fn equal_hashes_are_up_to_date_in_any_case() {
        assert_eq!(refresh_needed(Some(A), Some(A)), RefreshDecision::UpToDate);
        let upper = A.to_ascii_uppercase();
        assert_eq!(
            refresh_needed(Some(&upper), Some(A)),
            RefreshDecision::UpToDate
        );
    }

    #[test]
    fn different_hashes_refresh() {
        assert_eq!(refresh_needed(Some(A), Some(B)), RefreshDecision::Refresh);
    }

    #[test]
    fn a_recorded_value_that_is_no_hash_refreshes() {
        assert_eq!(
            refresh_needed(Some("abc"), Some("abc")),
            RefreshDecision::Refresh
        );
    }
}
