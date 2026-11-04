//! The usage counts' desktop half (decision record 0036, "The desktop"). The counts themselves,
//! their files and the sender's schedule are `clauseo-counts`; the host counts its own call sites
//! into the same handle (`HostConfig::with_counts`). Here:
//!
//! - Opening ([`open`]), on the host's start thread before the host starts (`lib.rs`): with
//!   `privacy.counts` after policy, whether `<data_dir>/first_act_done` exists, the envelope
//!   values ([`about`]) and a seed from the OS random source. A machine the schema cannot
//!   describe (an OS version or a CPU it has no value for) gets no counts this run: nothing is
//!   counted, saved, sent or deleted.
//! - The tick task ([`spawn_ticker`]): one task on Tauri's runtime that every 60 s saves the
//!   counts and makes the POST that is due (`send_due`). Before each POST it reads
//!   `privacy.counts` after policy from the settings files again ([`effective_on`]), so a policy
//!   that appeared since start, or an off, sends nothing and deletes the counts folder. The exit
//!   saves once more ([`CountsState::save`]). The clock is read here and nowhere else.
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

use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use clauseo_counts::schema::Time;
use clauseo_counts::{Counts, Upload, send_due};
use clauseo_settings::{Effective, Locations};
use tauri::{Manager as _, Runtime, WindowEvent};

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
    let about = about(effective, exe)?;
    let data_dir = locations.data_dir();
    let consented = data_dir.join(clauseo_counts::FIRST_ACT).exists();
    Some(Counts::open(
        data_dir,
        about,
        effective.settings().privacy.counts,
        consented,
        clauseo_counts::os_seed(),
    ))
}

/// `privacy.counts` after policy, read from the settings files and the policy sources now.
pub(crate) fn effective_on(locations: &Locations) -> bool {
    crate::load_effective(locations).settings().privacy.counts
}

/// Starts the task that ticks `counts` every 60 s and makes the POSTs that are due through
/// `upload`, reading `privacy.counts` from `locations`' settings before each.
pub(crate) fn spawn_ticker(counts: Counts, upload: HttpsUpload, locations: Locations) {
    drop(tauri::async_runtime::spawn(async move {
        loop {
            let now = now();
            tick_once(&counts, &upload, now, &|| effective_on(&locations)).await;
            async_io::Timer::after(TICK).await;
        }
    }));
}

/// One tick at `now`: saves, then makes the POST that is due, if any, asking `effective_on`
/// before it. A save that fails is tried again on the next tick.
pub async fn tick_once(
    counts: &Counts,
    upload: &dyn Upload,
    now: Time,
    effective_on: &(dyn Fn() -> bool + Sync),
) {
    if let Err(err) = counts.tick(now) {
        // The kind alone: the error's text could hold a path.
        tracing::debug!(kind = %err.kind(), "the usage counts were not all saved; the next tick tries again");
    }
    send_due(counts, upload, now, effective_on).await;
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
