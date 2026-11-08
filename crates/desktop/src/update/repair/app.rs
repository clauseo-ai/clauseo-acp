//! The repair's seams over the app, on Windows: the attempt's [`Stage`], the window's channel,
//! the marker, the windows' hand-over and return, and the bounded quit.

#![expect(
    clippy::disallowed_methods,
    reason = "the hand-over's real waits, on the repair's blocking thread: the announcement, the \
              windows' destruction (polled), and the quit's bound; the flow's tests stub them"
)]

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, Runtime};

use super::flow::Seams;
use super::windows::{Stage, main_window_class_exists};
use super::{HANDING_OVER, RepairNote, RepairState, RepairStep, marker};

/// How long "Windows will ask for permission" stays up before the windows go.
const ANNOUNCE: Duration = Duration::from_millis(1_500);

/// How long the windows may take to be destroyed before the launch is given up.
const HAND_OVER_WAIT: Duration = Duration::from_secs(5);

/// How long the quit after a launch may take before the process ends itself: well inside the
/// 10 s the installer's `/CLAUSEO_REPAIR` waits for Clauseo to be gone. The quit is the app's own
/// (`app.exit`, as an update's install: the agents, the host, the log's flush); with nothing
/// running it takes well under a second, but its bounds add up past 10 s with an agent that
/// ignores its cancel (`quit::EXIT_STOP_WAIT`). What is left ends with the process: the agents'
/// Job Objects kill on close, and the next start repairs a stop that did not finish.
pub(super) const EXIT_BOUND: Duration = Duration::from_secs(6);

/// The app's seams.
pub(super) struct AppSeams<R: Runtime> {
    pub(super) app: AppHandle<R>,
    pub(super) updates: PathBuf,
    pub(super) marker: PathBuf,
    pub(super) version: String,
    pub(super) steps: Channel<RepairStep>,
}

impl<R: Runtime> Seams for AppSeams<R> {
    type Stage = Stage;

    fn stage(&self, file: &str) -> std::io::Result<Stage> {
        Stage::create(&self.updates, &self.version, file)
    }

    fn discard(&self, stage: Stage) {
        stage.discard();
    }

    fn announce(&self, step: RepairStep) {
        // A window that went away hears nothing.
        drop(self.steps.send(step));
    }

    fn still_eligible(&self) -> bool {
        use clauseo_protocol::{HostQuery, QueryResult};
        let Some(bridge) = self.app.try_state::<crate::bridge::Bridge>() else {
            return false;
        };
        // On the repair's blocking thread: the host answers from its actor, at once.
        let asked =
            tauri::async_runtime::block_on(bridge.query(crate::MAIN_WINDOW, HostQuery::GetUpdate));
        matches!(asked, Ok(QueryResult::Update(view)) if !super::plan::update_refuses(&view.state))
    }

    fn mark(&self) -> std::io::Result<()> {
        marker::write(&self.marker, &self.version, super::now_secs())
    }

    fn unmark(&self) {
        marker::remove(&self.marker);
    }

    fn hand_over(&self) -> bool {
        std::thread::sleep(ANNOUNCE);
        // Closing the last window asks the app to exit; lib.rs holds that while this is set.
        HANDING_OVER.store(true, Ordering::Release);
        for window in self.app.webview_windows().values() {
            if window.destroy().is_err() {
                tracing::debug!("repair: a window was not destroyed");
            }
        }
        let until = Instant::now() + HAND_OVER_WAIT;
        loop {
            if self.app.webview_windows().is_empty() && !main_window_class_exists() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn launch(&self, stage: &Stage, args: &[&str]) -> std::io::Result<()> {
        stage.launch(args)
    }

    fn close(&self, stage: Stage) {
        drop(stage);
    }

    fn bring_back(&self) {
        if let Some(state) = self.app.try_state::<std::sync::Arc<RepairState>>() {
            state.set_note(RepairNote::LaunchFailed);
        }
        // On the main thread, which owns the windows (the frame's subclass and child window are
        // made on the window's thread): after any exit request the last window's closing
        // queued there, which the flag still holds, then the flag cleared and the window built.
        // A window that can't be built ends the app rather than leave it running unseen.
        let app = self.app.clone();
        let queued = self.app.run_on_main_thread(move || {
            HANDING_OVER.store(false, Ordering::Release);
            if app.get_webview_window(crate::MAIN_WINDOW).is_none()
                && crate::open_main_window(&app).is_err()
            {
                tracing::warn!(
                    target: super::super::install::LIFECYCLE,
                    "repair: the main window was not built again; quitting"
                );
                app.exit(0);
            }
        });
        if queued.is_err() {
            HANDING_OVER.store(false, Ordering::Release);
            tracing::warn!(
                target: super::super::install::LIFECYCLE,
                "repair: the main thread was not reached; quitting"
            );
            self.app.exit(0);
        }
    }

    fn quitting(&self) -> bool {
        self.app
            .try_state::<std::sync::Arc<crate::quit::Exit>>()
            .is_some_and(|exit| exit.stopping.load(Ordering::Acquire))
    }

    fn exit(&self) {
        let app = self.app.clone();
        let watchdog = std::thread::Builder::new()
            .name("clauseo-repair-exit".to_owned())
            .spawn(move || {
                std::thread::sleep(EXIT_BOUND);
                tracing::warn!(
                    target: super::super::install::LIFECYCLE,
                    "repair: the quit outran its bound; ending the process"
                );
                crate::host_log::flush_app(&app);
                std::process::exit(0);
            });
        if watchdog.is_err() {
            tracing::warn!("repair: no thread to bound the quit");
        }
        self.app.exit(0);
    }
}
