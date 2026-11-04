//! `InstallBrowser` and `CancelBrowserInstall` on the actor (S27, decision record 0029): the
//! download itself runs on a thread of its own (`browser::install::run`), one at a time, and
//! posts each state back here, where it goes to every app subscriber as
//! `HostEvent::BrowserInstall`. The actor never waits on it.
//!
//! - `InstallBrowser` starts the thread and answers `Done`; while one runs it answers `Done` and
//!   does nothing, since the running one's events already tell the app. While a cancelled one is
//!   still ending, it answers `Done` and a new download starts once that one's `Failed` is out.
//!   Neither command stores a receipt.
//! - `CancelBrowserInstall` sets the running download's [`Cancel`]; with none running it does
//!   nothing.
//! - The latest state goes into every app catch-up (`current`), so a window that lagged,
//!   resubscribed or reloaded hears where the download stands, its end included.
//! - Shutdown cancels the download too and waits for its last state ([`busy`]), so the tool it
//!   ran has ended and its working folder is gone before the host closes.
//! - At start, a thread removes the working folders (`browsers/.partial-*`) a crash left; the
//!   first download waits for it, so it never removes a new one.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, CommandOutcome, HostEvent};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::browser::chrome::{BROWSERS_DIR, ChromeRoots, Platform};
use crate::browser::install::{self, Cancel, InstallTools, Job, archive};
use crate::error::Error;

/// The download thread's name.
const THREAD_NAME: &str = "clauseo-browser-install";

/// The start-up sweep thread's name.
const SWEEP_THREAD_NAME: &str = "clauseo-browser-sweep";

/// The download's state on the actor.
pub(crate) struct BrowserInstall {
    tools: Arc<dyn InstallTools>,
    /// The start-up sweep, until the first download takes it to wait for.
    sweep: Option<JoinHandle<()>>,
    /// The running download.
    running: Option<Running>,
    /// The latest state published: the running download's last `Downloading` or `Unpacking`,
    /// then its `Done` or `Failed` until the next download starts. In every app catch-up, so a
    /// window that lagged, resubscribed or reloaded hears where the download stands.
    latest: Option<BrowserInstallState>,
    /// An `InstallBrowser` came while the running download was cancelled but not yet ended: a
    /// new download starts once the cancelled one's last state is in.
    restart: bool,
}

/// A running download.
struct Running {
    cancel: Cancel,
    thread: JoinHandle<()>,
}

impl BrowserInstall {
    /// No download yet, with `tools` (production: `SystemTools`).
    pub(crate) fn new(tools: Arc<dyn InstallTools>) -> Self {
        Self {
            tools,
            sweep: None,
            running: None,
            latest: None,
            restart: false,
        }
    }

    /// No download yet, with the wiring's tools (`HostConfig`'s test seam), or `SystemTools`
    /// when it names none.
    pub(crate) fn for_wiring(wiring: &crate::config::LaunchWiring) -> Self {
        let tools = wiring.install_tools.clone();
        Self::new(tools.unwrap_or_else(|| Arc::new(install::SystemTools)))
    }
}

/// The machine and the app data folder the download is for: the launch prep's Chrome roots, so
/// the next session's search looks where the download put it.
fn place(actor: &Actor) -> (Platform, PathBuf) {
    match &actor.wiring.chrome_roots {
        Some(ChromeRoots {
            platform, app_data, ..
        }) => (*platform, app_data.clone()),
        None => (
            Platform::current(),
            actor.effective.data_dir().to_path_buf(),
        ),
    }
}

/// Starts the sweep of leftover working folders, off the actor.
pub(super) fn start(actor: &mut Actor) {
    let browsers = place(actor).1.join(BROWSERS_DIR);
    let swept = std::thread::Builder::new()
        .name(SWEEP_THREAD_NAME.to_owned())
        .spawn(move || archive::sweep_partials(&browsers));
    match swept {
        Ok(thread) => actor.browser_install.sweep = Some(thread),
        Err(error) => tracing::warn!(%error, "the leftover download sweep did not start"),
    }
}

/// `InstallBrowser`: the thread starts and the command answers; while one runs, a no-op.
pub(super) fn install(actor: &mut Actor, ctx: &CommandCtx) {
    let result = begin(actor).map(|()| CommandOutcome::Done);
    actor.finish_command(ctx.id, result);
}

fn begin(actor: &mut Actor) -> Result<(), Error> {
    if let Some(running) = &actor.browser_install.running {
        if running.cancel.is_cancelled() {
            // The lawyer asked again after a cancel: start afresh once that one has ended.
            tracing::info!("a cancelled download is still ending; a new one starts after it");
            actor.browser_install.restart = true;
        } else {
            tracing::info!("a Chrome for Testing download is already running; nothing started");
        }
        return Ok(());
    }
    let (platform, app_data) = place(actor);
    let job = Job {
        platform,
        app_data,
        random: uuid::Uuid::now_v7().simple().to_string(),
    };
    let cancel = Cancel::new();
    let tools = Arc::clone(&actor.browser_install.tools);
    let sweep = actor.browser_install.sweep.take();
    let post = actor.post.clone();
    let thread_cancel = cancel.clone();
    let thread = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            if let Some(sweep) = sweep {
                drop(sweep.join());
            }
            let last = LastState::new(post.clone());
            let mut report = |state| {
                let _gone = post.try_send(Input::BrowserInstall(state));
            };
            let result = install::run(&job, tools.as_ref(), &thread_cancel, &mut report);
            last.send(result);
        })
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    actor.browser_install.running = Some(Running { cancel, thread });
    // The last download's end is no longer where things stand.
    actor.browser_install.latest = None;
    Ok(())
}

/// Posts the download's last state once: `Done` or its failure, or, if the thread panics
/// before it does, a failure from `Drop`, so the actor never waits on a download that is gone.
struct LastState {
    post: Option<async_channel::Sender<Input>>,
}

impl LastState {
    fn new(post: async_channel::Sender<Input>) -> Self {
        Self { post: Some(post) }
    }

    fn send(mut self, result: Result<PathBuf, install::Failure>) {
        let state = match result {
            Ok(folder) => {
                tracing::info!(folder = %folder.display(), "Chrome for Testing is installed");
                BrowserInstallState::Done
            }
            Err(failure) => {
                tracing::warn!(code = ?failure.code, message = %failure.message,
                    "the Chrome for Testing download failed");
                BrowserInstallState::Failed {
                    code: failure.code,
                    message: failure.message,
                }
            }
        };
        if let Some(post) = self.post.take() {
            let _gone = post.try_send(Input::BrowserInstall(state));
        }
    }
}

impl Drop for LastState {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            let state = BrowserInstallState::Failed {
                code: BrowserInstallFailure::Unpack,
                message: "the download thread ended without a result".to_owned(),
            };
            let _gone = post.try_send(Input::BrowserInstall(state));
        }
    }
}

/// `CancelBrowserInstall`: sets the running download's cancel, and drops a restart asked for
/// after an earlier cancel. Answers `Done`.
pub(super) fn cancel(actor: &mut Actor, ctx: &CommandCtx) {
    actor.browser_install.restart = false;
    if let Some(running) = &actor.browser_install.running {
        running.cancel.cancel();
    }
    actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
}

/// A state the thread posted: to every app subscriber; the last one ends the download.
pub(super) fn on_state(actor: &mut Actor, state: BrowserInstallState) {
    let last = matches!(
        state,
        BrowserInstallState::Done | BrowserInstallState::Failed { .. }
    );
    actor.browser_install.latest = Some(state.clone());
    super::app::publish(actor, &HostEvent::BrowserInstall { state });
    if last && let Some(running) = actor.browser_install.running.take() {
        // It posted its last state as its last act, so this join does not wait.
        if running.thread.join().is_err() {
            tracing::warn!("the download thread panicked");
        }
        // An Install that came while this one was ending after a cancel: its turn now, after
        // the cancelled one's `Failed` went out.
        // Never once the host is stopping.
        let serving = matches!(actor.phase, super::state::HostPhase::Serving);
        if std::mem::take(&mut actor.browser_install.restart)
            && serving
            && let Err(error) = begin(actor)
        {
            tracing::warn!(%error, "the download asked for after a cancel did not start");
        }
    }
}

/// The app catch-up's part: the latest state, whole, when there is one.
pub(super) fn current(actor: &Actor) -> Option<HostEvent> {
    let state = actor.browser_install.latest.clone()?;
    Some(HostEvent::BrowserInstall { state })
}

/// Shutdown: the running download is cancelled, and no restart follows it.
pub(super) fn shutdown(actor: &mut Actor) {
    actor.browser_install.restart = false;
    if let Some(running) = &actor.browser_install.running {
        running.cancel.cancel();
    }
}

/// Whether a download still runs: shutdown waits for its last state.
pub(super) fn busy(actor: &Actor) -> bool {
    actor.browser_install.running.is_some()
}
