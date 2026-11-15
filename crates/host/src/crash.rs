//! Crash reports' host half (decision record 0060): the panic hook's record and the native crash
//! handler. The reports, their queue and their sending are `clauseo-counts`' [`Crashes`]; desktop
//! opens it with counts, ingests once, then calls [`record_crashes`] before the host starts.
//!
//! - Panics: the hook `log_file::install` sets records the first panic of the process while
//!   recording ([`Crashes::prepare_panic`]), its file made relative and its message never kept,
//!   and writes it on the hook's helper thread with the log line, waiting a bounded time. Only a
//!   build that aborts on panic (`panic = "abort"`, the release profile) records: there every
//!   panic ends the app. A build that unwinds may catch one and carry on, so it records none.
//! - Native crashes ([`native`]): a fault the OS raises (SIGSEGV, SIGBUS, SIGILL, SIGFPE from the
//!   kernel, SIGABRT; on Windows an exception no handler took) appends its code to the record
//!   opened at start, then the previous handler runs. A kill, a power loss or a shutdown raises
//!   none of these, so it is never a report. The abort that follows a panic is not a second one.
//! - [`crash_switch`]: what a settings view says about sending.

use std::panic::PanicHookInfo;
use std::sync::OnceLock;

use clauseo_counts::schema::Time;
use clauseo_counts::{Crashes, Pending, Switch};
use clauseo_protocol::SettingsView;

use crate::clock::{Clock as _, SystemClock};

mod native;

/// The crash reports this process records into, once desktop sets them.
static CRASHES: OnceLock<Crashes> = OnceLock::new();

/// The settings key whose policy lock keeps crash reports from being sent.
const CRASH_REPORTS_KEY: &str = "privacy.crash_reports";

/// Records this process's crashes into `crashes` from now on: the panic hook's, and the native
/// handler's, which this installs with its record opened (when `crashes` is recording). Call
/// once, after the first [`Crashes::ingest`]; a second call changes nothing. Returns whether the
/// native handler was installed.
pub fn record_crashes(crashes: &Crashes) -> bool {
    if CRASHES.set(crashes.clone()).is_err() {
        return false;
    }
    match crashes.open_native(now()) {
        Ok(Some(file)) => native::install(crashes.clone(), file),
        Ok(None) => false,
        Err(err) => {
            tracing::debug!(kind = %err.kind(), "the native crash record did not open");
            false
        }
    }
}

/// The panic's report, made on the panicking thread, for the hook's helper thread to write.
pub(crate) fn on_panic(info: &PanicHookInfo<'_>) -> Option<Pending> {
    if !cfg!(panic = "abort") {
        return None;
    }
    let crashes = CRASHES.get()?;
    let location = info.location();
    let current = std::thread::current();
    crashes.prepare_panic(
        location.map(std::panic::Location::file),
        location.map_or(0, std::panic::Location::line),
        current.name(),
        now(),
    )
}

/// What `view` says: counts after policy, and whether policy locks crash reports off (a lock
/// on a settings key that can only be off, `privacy.crash_reports` or a section lock over it).
pub fn crash_switch(view: &SettingsView) -> Switch {
    switch_of(view.privacy.counts, &view.locked)
}

/// [`crash_switch`] from its two parts.
fn switch_of(counts: bool, locked: &[String]) -> Switch {
    let locked_off = locked.iter().any(|key| {
        key == CRASH_REPORTS_KEY
            || (key.ends_with('.') && CRASH_REPORTS_KEY.starts_with(key.as_str()))
    });
    Switch { counts, locked_off }
}

/// UTC now, from the host's one clock.
fn now() -> Time {
    let millis = SystemClock.now().wall.as_millis();
    Time::from_unix_secs(u64::try_from(millis / 1000).unwrap_or(0))
}

#[cfg(test)]
mod tests;
