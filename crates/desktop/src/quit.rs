//! The quit's pieces (`lib.rs`'s `ExitRequested` stops the app through `stop_host_then_exit`,
//! and its `RunEvent::Exit` through [`stop_on_exit`] when no stop ran before it): the agents'
//! installs and sign-ins are shut down first (`setup`, `BackendCell::shutdown`), then the host
//! stops ([`stop_in_order`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use clauseo_host::HostExit;
use tauri::{AppHandle, Manager as _, Runtime};

use crate::bridge::Bridge;
use crate::{counts, host_log, host_start, setup};

/// Where exit stands: a stopper started, and the host stopped (so the next request exits).
/// `stopped` is set through [`Exit::mark_stopped`] only, which also wakes
/// [`Exit::wait_stopped`].
#[derive(Default)]
pub(crate) struct Exit {
    pub(crate) stopping: AtomicBool,
    pub(crate) stopped: AtomicBool,
    /// Set by [`stop_on_exit`] before it waits: the main thread is held in `RunEvent::Exit`, so
    /// the stopper calls nothing that needs it (the update's install:
    /// tauri-runtime-wry's getters send to the main thread and wait with no bound).
    pub(crate) exiting_on_main: AtomicBool,
    /// `stopped`, under the lock the waiters sleep on.
    done: Mutex<bool>,
    ended: Condvar,
}

impl Exit {
    /// Records that the stop sequence has ended, and wakes every [`Exit::wait_stopped`].
    pub(crate) fn mark_stopped(&self) {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        *done = true;
        self.stopped.store(true, Ordering::Release);
        drop(done);
        self.ended.notify_all();
    }

    /// Waits at most `bound` for [`Exit::mark_stopped`]; true once it has run.
    pub(crate) fn wait_stopped(&self, bound: Duration) -> bool {
        let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        let (done, _timed_out) = self
            .ended
            .wait_timeout_while(done, bound, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *done
    }
}

/// The setup backend the quit shuts down: `Agents`' installs and sign-ins.
pub(crate) struct SetupStop(pub(crate) Arc<setup::BackendCell>);

/// How long the quit waits for the agents' jobs to end and their children to be killed.
pub(crate) const AGENTS_STOP_WAIT: Duration = Duration::from_secs(3);

/// How long `RunEvent::Exit` waits for the stop sequence when nothing stopped the host before it
/// (macOS's quit: the app menu's Quit, the Dock, logout and shutdown; [`stop_on_exit`]):
/// [`AGENTS_STOP_WAIT`] for the agents' installs and sign-ins, then 10 s for the host, whose
/// agents' ladders run side by side (stdin 2 s, SIGTERM 3 s, SIGKILL 2 s: `clauseo-process`)
/// with 3 s for their pipes and the host thread's join. Past it the event returns and the OS
/// ends the process with what is left.
pub(crate) const EXIT_STOP_WAIT: Duration = Duration::from_secs(3 + 10);

/// The quit's two steps in their order: `agents` shuts `Agents` down (its answer says whether
/// its jobs ended in time), then `host` stops the host. A job still running is logged, and the
/// host stops all the same. Returns `agents`' answer.
pub(crate) fn stop_in_order(agents: impl FnOnce() -> bool, host: impl FnOnce()) -> bool {
    let stopped = agents();
    if !stopped {
        tracing::warn!("an agents job was still running at quit");
    }
    host();
    stopped
}

/// The stopper thread's steps (`lib.rs`'s `stop_host_then_exit`): the quit's order
/// ([`stop_in_order`]: `agents`, then `host`, which answers how the host ended), then `flush`
/// (the host log, 0047), then `save_counts`, and last `after_host` with how the host ended and
/// whether the agents' jobs all ended in time, which installs a downloaded update only if they
/// did (`update::after_host`, 0037).
pub(crate) fn stop_then_after_host(
    agents: impl FnOnce() -> bool,
    host: impl FnOnce() -> HostExit,
    flush: impl FnOnce(),
    save_counts: impl FnOnce(),
    after_host: impl FnOnce(HostExit, bool),
) {
    let mut ended = HostExit::Stopped;
    let agents_stopped = stop_in_order(agents, || ended = host());
    flush();
    save_counts();
    after_host(ended, agents_stopped);
}

/// The stop sequence, blocking: the agents' installs and sign-ins first, their children killed
/// (#386), then the host (waits for its start, shuts it down, joins it); then the log, the
/// counts and `after_host`, in that order (quit.rs).
pub(crate) fn stop_steps<R: Runtime>(app: &AppHandle<R>, after_host: impl FnOnce(HostExit, bool)) {
    let cell = app.try_state::<Bridge>().map(|bridge| bridge.cell());
    stop_then_after_host(
        || {
            app.try_state::<SetupStop>()
                .is_none_or(|setup| setup::SetupBackend::shutdown(&*setup.0, AGENTS_STOP_WAIT))
        },
        || cell.as_deref().map_or(HostExit::Stopped, host_start::stop),
        // The log's queued lines go to the file before the exit skips the drops.
        || host_log::flush_app(app),
        // After the host, whose last counts go in too.
        || {
            if let Some(counts) = app.try_state::<counts::CountsState>() {
                counts.save();
            }
        },
        after_host,
    );
}

/// What [`stop_on_exit_with`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitWait {
    /// The host had already stopped: nothing to do.
    AlreadyStopped,
    /// Nothing ran (no process handle held, no setup job): no stop, no wait.
    Idle,
    /// The stop ended within the bound (this call's stopper, or one already running).
    Stopped,
    /// The bound passed first: what was not stopped is left to the OS.
    TimedOut,
    /// No thread for the stopper.
    NoThread,
}

/// [`stop_on_exit`]'s order, apart from the app: `hide` hides every window first (the
/// window-state plugin has already saved, before this callback: tauri 2.12.4 `app.rs:2815`),
/// then [`Exit::exiting_on_main`] is set, then the first caller asks `idle` and, when something
/// runs, `start` starts the stopper (false: no thread for it); a later caller only waits. The
/// wait is at most `bound`. `flush` runs when no stopper will flush the log (idle, no thread,
/// timed out).
pub(crate) fn stop_on_exit_with(
    exit: &Arc<Exit>,
    bound: Duration,
    hide: impl FnOnce(),
    idle: impl FnOnce() -> bool,
    start: impl FnOnce(Arc<Exit>) -> bool,
    flush: impl FnOnce(),
) -> ExitWait {
    if exit.stopped.load(Ordering::Acquire) {
        return ExitWait::AlreadyStopped;
    }
    hide();
    exit.exiting_on_main.store(true, Ordering::Release);
    let first = exit
        .stopping
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if first {
        if idle() {
            flush();
            return ExitWait::Idle;
        }
        if !start(Arc::clone(exit)) {
            // No thread for it: the host's next start repairs what it left open.
            flush();
            return ExitWait::NoThread;
        }
    }
    if exit.wait_stopped(bound) {
        ExitWait::Stopped
    } else {
        tracing::warn!(
            "the stop at exit did not end in time: what it had not stopped is left running"
        );
        flush();
        ExitWait::TimedOut
    }
}

/// Hides every webview window, `main` and the settings window, so a quit looks done at once.
/// On the main thread tauri-runtime-wry runs the hide inline (2.12.2 `lib.rs:263-280`);
/// elsewhere it goes to the loop. A window that does not hide stays as it is.
pub(crate) fn hide_windows<R: Runtime>(app: &AppHandle<R>) {
    for window in app.webview_windows().values() {
        if window.hide().is_err() {
            tracing::debug!("a window did not hide at quit");
        }
    }
}

/// Whether the quit has nothing to wait for. It closes `clauseo_process`'s gate first
/// (`close_spawns`: no process starts in this process again, the host's agents included), then
/// asks that no process is held or starting (no agent, sign-in, install or plan reader:
/// `clauseo_process::live_handles`) and that `Agents` has no job (its shutdown with no wait
/// answers that every job ended; it closes `Agents` to new jobs, which the stop does first
/// anyway). The count is read again after it, for a job that ended its process meanwhile.
fn nothing_runs<R: Runtime>(app: &AppHandle<R>) -> bool {
    // The gate first, then the count: a start either is refused or was counted (process's
    // `live.rs`). The app is exiting, so nothing starts again either way.
    clauseo_process::close_spawns();
    clauseo_process::live_handles() == 0
        && app
            .try_state::<SetupStop>()
            .is_none_or(|setup| setup::SetupBackend::shutdown(&*setup.0, Duration::ZERO))
        && clauseo_process::live_handles() == 0
}

/// `RunEvent::Exit` with the host not stopped. On macOS the app menu's Quit (Cmd-Q, muda's
/// `terminate:`), the Dock's Quit, logout and shutdown all reach only this: tao 0.37 gives its
/// app delegate no `applicationShouldTerminate:`, and its `applicationWillTerminate:` sends
/// `LoopDestroyed`, which tauri-runtime-wry turns into `RunEvent::Exit` and never into
/// `ExitRequested`. A Windows logoff or shutdown reaches it as well (tao 0.37.1's
/// `WM_ENDSESSION`, `event_loop.rs:2332-2342`, which then calls `process::exit`). The process
/// ends when this returns. The windows hide first; then, when nothing runs ([`nothing_runs`]),
/// it returns at once (the counts saved and the log flushed), so an idle app never holds a
/// logoff. Else the stop sequence runs on its own thread, waited for at most
/// [`EXIT_STOP_WAIT`], with no install after it
/// ([`Exit::exiting_on_main`]: the plugin saved before this callback, and the OS is ending the
/// app, which a restart would fight). When a stopper already runs (a quit signal or a closed
/// window came first), this waits for it the same way.
pub(crate) fn stop_on_exit<R: Runtime>(app: &AppHandle<R>, exit: &Arc<Exit>) -> ExitWait {
    if exit.stopped.load(Ordering::Acquire) {
        return ExitWait::AlreadyStopped;
    }
    if let Some(counts) = app.try_state::<counts::CountsState>() {
        drop(counts.stop_ticker());
    }
    stop_on_exit_with(
        exit,
        EXIT_STOP_WAIT,
        || hide_windows(app),
        || {
            let idle = nothing_runs(app);
            if idle && let Some(counts) = app.try_state::<counts::CountsState>() {
                counts.save();
            }
            idle
        },
        |stopper_exit| {
            let stopper_app = app.clone();
            std::thread::Builder::new()
                .name("clauseo-host-stop".to_owned())
                .spawn(move || {
                    stop_steps(&stopper_app, |_ended, _agents_stopped| {});
                    stopper_exit.mark_stopped();
                })
                .is_ok()
        },
        || host_log::flush_app(app),
    )
}

/// The `ExitRequested` stopper's `after_host` (`update::after_host`: the install), unless
/// [`stop_on_exit`] holds the main thread: then it does not run, and a downloaded update
/// installs at the next start.
pub(crate) fn after_host_unless_exiting(
    exit: &Exit,
    ended: HostExit,
    agents_stopped: bool,
    after_host: impl FnOnce(HostExit, bool),
) {
    if exit.exiting_on_main.load(Ordering::Acquire) {
        if let HostExit::Install(downloaded) = &ended {
            tracing::info!(
                target: "clauseo_desktop::lifecycle",
                version = downloaded.version.as_str(),
                "the app is exiting: the update installs at the next start"
            );
        }
        return;
    }
    after_host(ended, agents_stopped);
}

/// `ExitRequested`, on the main thread, once `prevent_exit` holds the exit. The first request
/// `save`s the window state here (the plugin's save: its getters run inline on the main thread,
/// and it holds the plugin's cache lock while they run, so it is never called from another
/// thread, where it could wait on a main thread blocked on that lock), then `hide`s the
/// windows, then `start`s the stopper (`lib.rs`'s `stop_host_then_exit`). Every quit that
/// installs an update starts here: the host's `HostExit::Install` asks for the exit
/// (`update.rs`'s `watching`). A later request does nothing: the stopper runs. Returns
/// whether this was the first.
pub(crate) fn on_exit_requested(
    exit: &Exit,
    save: impl FnOnce(),
    hide: impl FnOnce(),
    start: impl FnOnce(),
) -> bool {
    let first = exit
        .stopping
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if first {
        save();
        hide();
        start();
    }
    first
}

/// The single-instance callback: a second start raises the main window, unless a quit is
/// stopping the app (its windows are hidden and it is about to end), when it is only logged;
/// the new instance has already handed over and exits either way. Returns whether it raised.
pub(crate) fn on_second_start<R: Runtime>(app: &AppHandle<R>, exit: &Exit) -> bool {
    if exit.stopping.load(Ordering::Acquire) {
        tracing::info!("a second start during the quit; start Clauseo again once it has closed");
        return false;
    }
    crate::raise_main_window(app);
    true
}

#[cfg(test)]
mod tests {
    mod exit;

    use std::path::PathBuf;
    use std::sync::Mutex;

    use std::sync::mpsc::Receiver;

    use clauseo_host::{Downloaded, HostExit, Opaque, UpdaterError};

    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use super::{Exit, stop_in_order, stop_then_after_host};
    use crate::bridge::lock;
    use crate::update::install::{self, After, Steps, Watch};

    /// What `after_host` heard, as a step's name.
    fn heard(ended: &HostExit) -> String {
        match ended {
            HostExit::Install(downloaded) => format!("after_host install {}", downloaded.version),
            _ => "after_host stopped".to_owned(),
        }
    }

    /// The stopper's steps, each recording its name; `agents` answers `ended_in_time` and the
    /// host ends as `ended`.
    fn run_stopper(ended_in_time: bool, ended: HostExit) -> Vec<String> {
        let steps = Mutex::new(Vec::new());
        let step = |name: &str| lock(&steps).push(name.to_owned());
        stop_then_after_host(
            || {
                step("agents");
                ended_in_time
            },
            || {
                step("host");
                ended
            },
            || step("flush"),
            || step("counts"),
            |ended, _agents_stopped| step(&heard(&ended)),
        );
        steps.into_inner().unwrap()
    }

    /// The install's steps, recorded; its install answers at once.
    #[derive(Default)]
    struct InstallSteps(Vec<&'static str>);

    impl Steps for InstallSteps {
        fn installable(&self, _downloaded: &Downloaded) -> bool {
            true
        }
        fn crash_clean_exit(&mut self) {
            self.0.push("crash clean exit");
        }
        fn flush_log(&mut self) {
            self.0.push("flush");
        }
        fn start_install(&mut self, _downloaded: Downloaded) -> Receiver<Result<(), UpdaterError>> {
            self.0.push("install");
            let (answer, answered) = std::sync::mpsc::channel();
            answer.send(Ok(())).unwrap();
            answered
        }
        fn app_in_place(&self) -> bool {
            true
        }
    }

    /// A download, as the host hands it back.
    fn downloaded() -> Downloaded {
        Downloaded {
            version: "0.4.1".to_owned(),
            path: PathBuf::from("updates/0.4.1/Clauseo-setup.exe"),
            handle: Opaque::new(()),
        }
    }

    /// The stopper over the real install decision: `agents` answers `ended_in_time`, the host
    /// stops for an install; returns what the app does next and the install's steps.
    fn install_after_quit(ended_in_time: bool) -> (Option<After>, Vec<&'static str>) {
        let mut install_steps = InstallSteps::default();
        let mut after = None;
        stop_then_after_host(
            || ended_in_time,
            || HostExit::Install(downloaded()),
            || {},
            || {},
            |ended, agents_stopped| {
                after = Some(install::after_host(
                    ended,
                    agents_stopped,
                    &mut install_steps,
                    Watch::INSTALL,
                ));
            },
        );
        (after, install_steps.0)
    }

    #[test]
    fn an_agents_job_still_running_at_quit_skips_the_install_and_restarts() {
        let (after, steps) = install_after_quit(false);
        assert_eq!(after, Some(After::Restart));
        assert_eq!(steps, ["flush"]);
        let (after, steps) = install_after_quit(true);
        assert_eq!(after, Some(After::Restart));
        assert_eq!(steps, ["crash clean exit", "flush", "install", "flush"]);
    }

    #[test]
    fn a_wait_for_the_stop_ends_when_another_thread_marks_it_stopped() {
        let exit = Arc::new(Exit::default());
        // Nothing marked it: the wait gives up at its bound.
        assert!(!exit.wait_stopped(Duration::ZERO));
        assert!(!exit.stopped.load(Ordering::Acquire));
        let marker = Arc::clone(&exit);
        let marked = std::thread::spawn(move || marker.mark_stopped());
        assert!(exit.wait_stopped(Duration::from_secs(120)));
        assert!(exit.stopped.load(Ordering::Acquire));
        marked.join().unwrap();
        // Once marked, every later wait answers at once.
        assert!(exit.wait_stopped(Duration::ZERO));
    }

    #[test]
    fn the_agents_shut_down_before_the_host_stops() {
        let steps = Mutex::new(Vec::new());
        let _stopped = stop_in_order(
            || {
                lock(&steps).push("agents");
                true
            },
            || lock(&steps).push("host"),
        );
        assert_eq!(*lock(&steps), ["agents", "host"]);
    }

    #[test]
    fn a_job_still_running_at_quit_does_not_keep_the_host_up() {
        let steps = Mutex::new(Vec::new());
        let stopped = stop_in_order(
            || {
                lock(&steps).push("agents");
                false
            },
            || lock(&steps).push("host"),
        );
        assert_eq!(*lock(&steps), ["agents", "host"]);
        assert!(!stopped);
    }

    #[test]
    fn the_install_runs_only_after_the_quit_s_order_the_log_flush_and_the_counts_save() {
        let downloaded = Downloaded {
            version: "0.4.1".to_owned(),
            path: PathBuf::from("updates/0.4.1/Clauseo-setup.exe"),
            handle: Opaque::new(()),
        };
        assert_eq!(
            run_stopper(true, HostExit::Install(downloaded)),
            [
                "agents",
                "host",
                "flush",
                "counts",
                "after_host install 0.4.1"
            ]
        );
    }

    #[test]
    fn a_plain_stop_with_a_job_still_running_reaches_after_host_as_stopped() {
        assert_eq!(
            run_stopper(false, HostExit::Stopped),
            ["agents", "host", "flush", "counts", "after_host stopped"]
        );
    }
}
