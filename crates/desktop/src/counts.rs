//! The usage counts' desktop half (decision record 0036, "The desktop"). The counts themselves,
//! their files and the sender's schedule are `clauseo-counts`; the host counts its own call sites
//! into the same handle (`HostConfig::with_counts`). Here:
//!
//! - Opening ([`open`]), on the host's start thread before the host starts (`lib.rs`): with
//!   `privacy.counts` after policy ([`counts_on`]: off when the user's settings file did not
//!   read whole, since the default would turn counts back on), whether
//!   `<data_dir>/first_act_done` exists, the envelope values ([`about`]) and a seed from the OS
//!   random source. A machine the schema cannot describe (an OS version or a CPU it has no value
//!   for) gets no counts this run: nothing is counted, saved, sent or deleted.
//! - The tick task ([`spawn_ticker`]): one task on Tauri's runtime that every 60 s saves the
//!   counts (on a blocking thread) and makes the POST that is due (`send_due`). Just before, it
//!   asks the host for `privacy.counts` after policy ([`host_counts_on`], the value the host
//!   switches by), so an off sends nothing and deletes the counts folder. A host that did not
//!   start, has shut down or does not answer gives no value, and the tick then changes nothing:
//!   no switch, no POST. An off read before the switch last moved is stale and turns nothing off
//!   ([`tick_once`]). The exit stops the task before it stops the host
//!   ([`CountsState::stop_ticker`]), then saves once more ([`CountsState::save`]). The clock is
//!   read here and nowhere else.
//! - `time_open`: the foreground time of the `main` and `settings` windows, through
//!   `Counts::set_foreground` ([`on_window_event`]).
//! - `count_ui`'s events ([`UiCount`]): only what the webview alone sees.
//! - The upload ([`upload`]): HTTPS only, to the endpoint of the build.
//!
//! `app_opened` (`cold`) is the host's: it counts it once it has started (`actor/counting.rs`),
//! so desktop adds none. R23 adds `after_update`.

mod about;
mod ui;
mod upload;

use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use clauseo_counts::schema::Time;
use clauseo_counts::{About, Counts, Upload, send_due};
use clauseo_protocol::{HostQuery, Principal, QueryResult};
use clauseo_settings::{Effective, Locations, Origin, WarningKind};
use tauri::async_runtime::JoinHandle;
use tauri::{Manager as _, Runtime, WindowEvent};

use crate::host_start::HostCell;

pub use about::about;
pub(crate) use ui::UiCount;
pub(crate) use upload::HttpsUpload;

/// How often the task ticks.
const TICK: Duration = Duration::from_secs(60);

/// The counts once open, and which of the app's windows has the focus. Managed from setup on,
/// before any window exists; the counts arrive from the host's start thread.
#[derive(Debug, Default)]
pub(crate) struct CountsState {
    counts: OnceLock<Counts>,
    /// Whether `main` and `settings` have the focus. Its lock also orders the counts' arrival
    /// against focus changes, so neither misses the other.
    focus: Mutex<[bool; 2]>,
    /// The tick task, once started, and whether the exit has stopped it.
    ticker: Mutex<Ticker>,
}

/// The tick task's place in [`CountsState`].
#[derive(Debug, Default)]
struct Ticker {
    task: Option<JoinHandle<()>>,
    stopped: bool,
}

impl CountsState {
    /// Puts `counts` in place (once) and tells them whether a window has the focus already.
    pub(crate) fn start(&self, counts: Counts) {
        let focus = self.focus.lock().unwrap_or_else(PoisonError::into_inner);
        if self.counts.set(counts).is_ok()
            && let Some(counts) = self.counts.get()
        {
            counts.set_foreground(focus.contains(&true), now());
        }
    }

    /// The window `label` gained or lost the focus.
    fn focused(&self, label: &str, focused: bool) {
        let place = match label {
            crate::MAIN_WINDOW => 0,
            crate::settings_window::SETTINGS_WINDOW => 1,
            _ => return,
        };
        let mut focus = self.focus.lock().unwrap_or_else(PoisonError::into_inner);
        let before = focus.contains(&true);
        if let Some(slot) = focus.get_mut(place) {
            *slot = focused;
        }
        let after = focus.contains(&true);
        if before != after
            && let Some(counts) = self.counts.get()
        {
            counts.set_foreground(after, now());
        }
    }

    /// Counts one of the webview's events. Before the counts open (they open before the host
    /// starts, and `count_ui` waits for the host's start) or without counts, nothing.
    pub(crate) fn count(&self, event: UiCount) {
        let Some(counts) = self.counts.get() else {
            tracing::debug!("a webview count with no usage counts to go to");
            return;
        };
        counts.bump(event.event());
    }

    /// Starts the tick task with `spawn`, unless the exit has stopped it already.
    pub(crate) fn run_ticker(&self, spawn: impl FnOnce() -> JoinHandle<()>) {
        let mut ticker = self.ticker.lock().unwrap_or_else(PoisonError::into_inner);
        if !ticker.stopped {
            ticker.task = Some(spawn());
        }
    }

    /// Stops the tick task for good (the exit calls it before it stops the host), and returns
    /// it, aborted, when it had started. A tick under way stops at its next wait; one already
    /// asking the host gets no answer from a host shutting down, and changes nothing.
    pub(crate) fn stop_ticker(&self) -> Option<JoinHandle<()>> {
        let mut ticker = self.ticker.lock().unwrap_or_else(PoisonError::into_inner);
        ticker.stopped = true;
        let task = ticker.task.take();
        if let Some(task) = &task {
            task.abort();
        }
        task
    }

    /// Saves the counts at exit.
    pub(crate) fn save(&self) {
        if let Some(counts) = self.counts.get()
            && let Err(err) = counts.tick(now())
        {
            tracing::debug!(kind = %err.kind(), "the usage counts were not all saved at exit");
        }
    }
}

/// Opens the counts of `locations`' data folder with the effective settings, for an app whose
/// executable is `exe`; none when the schema cannot describe this machine ([`about`]).
pub fn open(locations: &Locations, effective: &Effective, exe: Option<&Path>) -> Option<Counts> {
    Some(open_with(locations, effective, about(effective, exe)?))
}

/// [`open`] with the envelope given.
pub fn open_with(locations: &Locations, effective: &Effective, about: About) -> Counts {
    let data_dir = locations.data_dir();
    let consented = data_dir.join(clauseo_counts::FIRST_ACT).exists();
    Counts::open(
        data_dir,
        about,
        counts_on(effective),
        consented,
        clauseo_counts::os_seed(),
    )
}

/// `privacy.counts` after policy, for opening the counts: off too when the user's settings file
/// exists but could not be read, is not valid JSON, or holds a `privacy` value that does not
/// type. `Effective::load` takes the defaults then, and the default (on) would resume counts the
/// lawyer may have turned off; the other settings keep their defaults as usual.
pub fn counts_on(effective: &Effective) -> bool {
    let settings_file = effective.locations().settings_file();
    let damaged = effective.warnings().iter().any(|warning| {
        let ours = matches!(&warning.origin, Origin::File { path, .. } if path == settings_file);
        let lost = match &warning.kind {
            WarningKind::Unreadable { .. } | WarningKind::Syntax { .. } => true,
            WarningKind::InvalidValue { key, .. } => key.starts_with("privacy"),
            _ => false,
        };
        ours && lost
    });
    effective.settings().privacy.counts && !damaged
}

/// `privacy.counts` after policy as the host holds it now (`GetSettings`): the value its
/// `SetPrivacySettings` and `FirstAct` switch the counts by, answered between commands. None
/// when the host did not start, has shut down (`Closed`) or answers anything else: no value,
/// never an off.
pub(crate) async fn host_counts_on(cell: &HostCell) -> Option<bool> {
    let host = cell.host().await.ok()?;
    match host
        .query(Principal::LocalDesktop, HostQuery::GetSettings)
        .await
    {
        Ok(QueryResult::Settings(view)) => Some(view.privacy.counts),
        _ => None,
    }
}

/// Starts the task that ticks `counts` every 60 s and makes the POSTs that are due through
/// `upload`, asking the host in `cell` for `privacy.counts` before each. It never ends by
/// itself: the exit aborts it ([`CountsState::stop_ticker`]).
pub(crate) fn spawn_ticker(
    counts: Counts,
    upload: HttpsUpload,
    cell: Arc<HostCell>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            tick_once(&counts, &upload, now(), host_counts_on(&cell)).await;
            async_io::Timer::after(TICK).await;
        }
    })
}

/// One tick at `now`: saves on a blocking thread, then reads `effective_on` and makes the POST
/// that is due, if any. A save that fails is tried again on the next tick.
///
/// No value from `effective_on` (the host is gone or did not answer) ends the tick there: no
/// switch and no POST, so a host shutting down never reads as an off. The switch's generation
/// is taken before `effective_on` is read. An off read while the switch moved (the host turned
/// counts on, and its settings write had not landed when the read was answered) is older than
/// the switch, so it turns nothing off this tick.
pub async fn tick_once(
    counts: &Counts,
    upload: &dyn Upload,
    now: Time,
    effective_on: impl Future<Output = Option<bool>>,
) {
    let saving = counts.clone();
    match tauri::async_runtime::spawn_blocking(move || saving.tick(now)).await {
        Ok(Ok(())) => {}
        // The kind alone: the error's text could hold a path.
        Ok(Err(err)) => {
            tracing::debug!(kind = %err.kind(), "the usage counts were not all saved; the next tick tries again");
        }
        Err(err) => {
            tracing::debug!(%err, "the usage counts' save thread did not finish; the next tick tries again");
        }
    }
    let since = counts.switch_generation();
    let Some(on) = effective_on.await else {
        tracing::debug!(
            "no answer from the host about the usage counts: this tick sends and switches nothing"
        );
        return;
    };
    send_due(counts, upload, now, &|| {
        on || counts.switch_generation() != since
    })
    .await;
}

/// The focus of `main` or the `settings` window, for `time_open`.
pub(crate) fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    let focused = match event {
        WindowEvent::Focused(focused) => *focused,
        // A window closed while it had the focus may not say it lost it.
        WindowEvent::Destroyed => false,
        _ => return,
    };
    if let Some(state) = window.app_handle().try_state::<CountsState>() {
        state.focused(window.label(), focused);
    }
}

/// UTC now, the one clock read of the counts (`clauseo-counts` takes time as an input).
#[expect(
    clippy::disallowed_methods,
    reason = "the counts' clock: the tick task, the focus and the exit read it here alone"
)]
fn now() -> Time {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    Time::from_unix_secs(secs)
}

#[cfg(test)]
mod tests;
