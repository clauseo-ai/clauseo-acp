//! `InstallBrowser`, `CancelBrowserInstall` and the daily check on the actor (S27, decision
//! record 0029): the download itself runs on a thread of its own (`browser::install::run`), one
//! at a time, and posts each state back here, where it goes to every app subscriber as
//! `HostEvent::BrowserInstall`. The actor never waits on it.
//!
//! - `InstallBrowser` first asks the firm: with its Clauseo policy's `browser.allow_download`
//!   false (or a policy source unreadable, which locks it false) it publishes
//!   `Failed { firm_forbids }` and starts nothing. Otherwise it starts the thread and answers
//!   `Done`; the thread reads the firm's Chrome, Edge and Chromium policy first, the launch's
//!   rule (`browser::rd_policy`), and a block is `firm_forbids` too. While one runs it answers
//!   `Done` and does nothing, since the running one's events already tell the app. While a
//!   cancelled one is still ending, it answers `Done` and a new download starts once that one's
//!   `Failed` is out. Neither command stores a receipt. The lawyer's download waits out a
//!   dropped connection (`Offline`, then a new try every [`install::OFFLINE_RETRY`]).
//! - The daily check: a timer thread ticks at start (its check waits until the host serves, since
//!   start-up waits for no download to run) and then once a day while the host runs;
//!   each tick with a downloaded copy on disk runs the same thread, quietly: it updates the
//!   record of Stables seen, downloads when Stable is newer, and removes old copies no session
//!   runs. Quiet means it publishes nothing until it starts downloading (a check that finds the
//!   copy current, or fails before downloading, is only logged), and it does not wait out an
//!   offline spell: the next tick tries again. A copy more than one Stable release behind is
//!   not started by any launch meanwhile (`chrome::find`), and its update's `Downloading` says
//!   `behind`, so the app tells the lawyer.
//! - `CancelBrowserInstall` sets the running download's [`Cancel`]; with none running it does
//!   nothing.
//! - The latest state goes into every app catch-up (`current`), so a window that lagged,
//!   resubscribed or reloaded hears where the download stands, its end included.
//! - Shutdown cancels the download too and waits for its last state ([`busy`]), so the tool it
//!   ran has ended and its working folder is gone before the host closes; the timer stops.
//! - At start, a thread removes the working folders (`browsers/.partial-*`) a crash left; the
//!   first download waits for it, so it never removes a new one.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState, CommandOutcome, HostEvent};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::browser::chrome::{BROWSERS_DIR, ChromeRoots, Platform};
use crate::browser::install::prune::{self, InUse};
use crate::browser::install::{self, Cancel, InstallTools, Job, Outcome, archive, fresh};
use crate::browser::rd_policy::RdPolicy;
use crate::error::Error;

/// The download thread's name.
const THREAD_NAME: &str = "clauseo-browser-install";

/// The start-up sweep thread's name.
const SWEEP_THREAD_NAME: &str = "clauseo-browser-sweep";

/// The daily check's timer thread's name.
const TIMER_THREAD_NAME: &str = "clauseo-browser-check";

/// How often the daily check runs while the host runs (and once at start).
pub(crate) const CHECK_EVERY: Duration = Duration::from_hours(24);

/// What reaches the actor from the download's thread and the timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// A state to publish; `Done` and `Failed` end the run.
    State(BrowserInstallState),
    /// The run ended with nothing to publish: a quiet check that found the copy current, or
    /// failed before it downloaded anything.
    QuietEnd,
    /// The daily timer ticked, and a downloaded copy is on disk.
    Check,
}

/// The download's state on the actor.
pub(crate) struct BrowserInstall {
    tools: Arc<dyn InstallTools>,
    /// The sessions running a downloaded copy, which the prune leaves alone. Shared with every
    /// launch prep (`launch_checks::browser`).
    pub(crate) in_use: InUse,
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
    /// Dropped to stop the daily timer.
    timer: Option<std::sync::mpsc::Sender<()>>,
    /// A tick came during start-up: its check runs once the host serves, so a download never
    /// holds start-up back (start-up waits for no download to run).
    check_after_start: bool,
}

/// A running download.
struct Running {
    cancel: Cancel,
    /// Whether it is the daily check's, publishing nothing until it downloads. The lawyer's
    /// `InstallBrowser` while one runs clears it, so its states and end reach the app.
    quiet: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl BrowserInstall {
    /// No download yet, with `tools` (production: `SystemTools`).
    pub(crate) fn new(tools: Arc<dyn InstallTools>) -> Self {
        Self {
            tools,
            in_use: InUse::new(),
            sweep: None,
            running: None,
            latest: None,
            restart: false,
            timer: None,
            check_after_start: false,
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

/// Starts the sweep of leftover working folders and the daily check's timer, off the actor.
pub(super) fn start(actor: &mut Actor) {
    let (platform, app_data) = place(actor);
    let browsers = app_data.join(BROWSERS_DIR);
    let swept = {
        let browsers = browsers.clone();
        std::thread::Builder::new()
            .name(SWEEP_THREAD_NAME.to_owned())
            .spawn(move || archive::sweep_partials(&browsers))
    };
    match swept {
        Ok(thread) => actor.browser_install.sweep = Some(thread),
        Err(error) => {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "the leftover download sweep did not start"
            );
        }
    }
    if platform.cft_name().is_none() {
        return;
    }
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let post = actor.post.clone();
    let timer = std::thread::Builder::new()
        .name(TIMER_THREAD_NAME.to_owned())
        .spawn(move || {
            loop {
                // Blocking, so a busy start-up's full input queue delays the tick, never drops it.
                if !fresh::installed(&browsers).is_empty()
                    && post
                        .send_blocking(Input::BrowserInstall(Step::Check))
                        .is_err()
                {
                    return;
                }
                match stopped.recv_timeout(CHECK_EVERY) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    _ => return,
                }
            }
        });
    match timer {
        Ok(_detached) => actor.browser_install.timer = Some(stop),
        Err(error) => {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "the daily browser check did not start"
            );
        }
    }
}

/// `InstallBrowser`: refused when the firm forbids it; else the thread starts and the command
/// answers; while one runs, a no-op.
pub(super) fn install(actor: &mut Actor, ctx: &CommandCtx) {
    if !actor.effective.settings().browser.allow_download {
        tracing::info!("the firm's policy forbids the Chrome for Testing download");
        let state = BrowserInstallState::Failed {
            code: BrowserInstallFailure::FirmForbids,
            message: "the firm's policy sets browser.allow_download false".to_owned(),
        };
        on_input(actor, Step::State(state));
        actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
        return;
    }
    let result = begin(actor, false).map(|()| CommandOutcome::Done);
    actor.finish_command(ctx.id, result);
}

/// The daily tick: a quiet check, unless one runs or the firm forbids downloads. During
/// start-up it waits for [`serving`].
fn check(actor: &mut Actor) {
    match actor.phase {
        super::state::HostPhase::Starting(_) => {
            actor.browser_install.check_after_start = true;
            return;
        }
        super::state::HostPhase::Stopping(_) => return,
        super::state::HostPhase::Serving => {}
    }
    if actor.browser_install.running.is_some() {
        return;
    }
    if !actor.effective.settings().browser.allow_download {
        tracing::info!("the daily browser check is skipped: the firm forbids the download");
        return;
    }
    if let Err(error) = begin(actor, true) {
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "the daily browser check did not start"
        );
    }
}

fn begin(actor: &mut Actor, quiet: bool) -> Result<(), Error> {
    if let Some(running) = &actor.browser_install.running {
        if running.cancel.is_cancelled() {
            // The lawyer asked again after a cancel: start afresh once that one has ended.
            tracing::info!("a cancelled download is still ending; a new one starts after it");
            actor.browser_install.restart = true;
        } else {
            // The lawyer's ask joins a quiet check: from now on the app hears it.
            running.quiet.store(false, Ordering::SeqCst);
            tracing::info!("a Chrome for Testing download is already running; nothing started");
        }
        return Ok(());
    }
    let (platform, app_data) = place(actor);
    let job = Job {
        platform,
        app_data,
        random: uuid::Uuid::now_v7().simple().to_string(),
        now: actor.now().wall.as_millis().div_euclid(1000),
    };
    let cancel = Cancel::new();
    let quiet = Arc::new(AtomicBool::new(quiet));
    let ctx = ThreadCtx {
        tools: Arc::clone(&actor.browser_install.tools),
        sweep: actor.browser_install.sweep.take(),
        post: actor.post.clone(),
        cancel: cancel.clone(),
        quiet: Arc::clone(&quiet),
        in_use: actor.browser_install.in_use.clone(),
        rd_policy: Arc::clone(&actor.wiring.rd_policy),
        allowed_anyway: actor
            .effective
            .settings()
            .browser
            .allow_when_remote_debugging_disabled,
    };
    let thread = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || ctx.run(&job))
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    actor.browser_install.running = Some(Running {
        cancel,
        quiet,
        thread,
    });
    // The last download's end is no longer where things stand.
    if !quiet_now(actor) {
        actor.browser_install.latest = None;
    }
    Ok(())
}

/// Whether the running download is a quiet check.
fn quiet_now(actor: &Actor) -> bool {
    actor
        .browser_install
        .running
        .as_ref()
        .is_some_and(|running| running.quiet.load(Ordering::SeqCst))
}

/// What the download's thread takes with it.
struct ThreadCtx {
    tools: Arc<dyn InstallTools>,
    sweep: Option<JoinHandle<()>>,
    post: async_channel::Sender<Input>,
    cancel: Cancel,
    quiet: Arc<AtomicBool>,
    in_use: InUse,
    rd_policy: Arc<RdPolicy>,
    allowed_anyway: bool,
}

impl ThreadCtx {
    /// The thread's whole work: the firm's browser policy, the run, the prune, the last state.
    fn run(self, job: &Job) {
        if let Some(sweep) = self.sweep {
            drop(sweep.join());
        }
        let last = LastState::new(self.post.clone(), Arc::clone(&self.quiet));
        if let Some(reason) = self.rd_policy.check(self.allowed_anyway) {
            last.send(Err(install::Failure::new(
                BrowserInstallFailure::FirmForbids,
                format!("the firm's browser policy leaves Clauseo's browser off: {reason:?}"),
            )));
            return;
        }
        let quiet = &self.quiet;
        let post = &self.post;
        let mut report = |state: BrowserInstallState| {
            // A quiet check speaks up once it downloads; its `Offline` never comes.
            if matches!(state, BrowserInstallState::Downloading { .. }) {
                quiet.store(false, Ordering::SeqCst);
            }
            if !quiet.load(Ordering::SeqCst) {
                let _gone = post.try_send(Input::BrowserInstall(Step::State(state)));
            }
        };
        let result = if self.quiet.load(Ordering::SeqCst) {
            install::run(job, self.tools.as_ref(), &self.cancel, &mut report)
        } else {
            install::run_until_online(
                job,
                self.tools.as_ref(),
                &self.cancel,
                &mut report,
                install::OFFLINE_RETRY,
            )
        };
        if result.is_ok() {
            let binary = job.platform.cft_binary().unwrap_or_default();
            prune::prune(&job.app_data.join(BROWSERS_DIR), &binary, &self.in_use);
        }
        last.send(result);
    }
}

/// Posts the download's last state once: `Done` or its failure (nothing for a quiet check that
/// did not download), or, if the thread panics before it does, a failure from `Drop`, so the
/// actor never waits on a download that is gone.
struct LastState {
    post: Option<async_channel::Sender<Input>>,
    quiet: Arc<AtomicBool>,
}

impl LastState {
    fn new(post: async_channel::Sender<Input>, quiet: Arc<AtomicBool>) -> Self {
        Self {
            post: Some(post),
            quiet,
        }
    }

    fn send(mut self, result: Result<Outcome, install::Failure>) {
        let quiet = self.quiet.load(Ordering::SeqCst);
        let step = match result {
            Ok(Outcome::Installed(_)) => {
                tracing::info!("Chrome for Testing is installed");
                Step::State(BrowserInstallState::Done)
            }
            Ok(Outcome::Current) if quiet => Step::QuietEnd,
            Ok(Outcome::Current) => Step::State(BrowserInstallState::Done),
            Err(failure) => {
                tracing::warn!(
                    code = ?failure.code,
                    message = %failure.message,
                    "the Chrome for Testing download failed"
                );
                if quiet {
                    Step::QuietEnd
                } else {
                    Step::State(BrowserInstallState::Failed {
                        code: failure.code,
                        message: failure.message,
                    })
                }
            }
        };
        if let Some(post) = self.post.take() {
            let _gone = post.try_send(Input::BrowserInstall(step));
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
            let _gone = post.try_send(Input::BrowserInstall(Step::State(state)));
        }
    }
}

/// The host now serves: a tick that came during start-up gets its check.
pub(super) fn serving(actor: &mut Actor) {
    if std::mem::take(&mut actor.browser_install.check_after_start) {
        check(actor);
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

/// What the thread or the timer posted. A state goes to every app subscriber; the last one (or
/// a quiet end) ends the download.
pub(super) fn on_input(actor: &mut Actor, step: Step) {
    let state = match step {
        Step::Check => return check(actor),
        Step::QuietEnd => None,
        Step::State(state) => Some(state),
    };
    let last = state.as_ref().is_none_or(|state| {
        matches!(
            state,
            BrowserInstallState::Done | BrowserInstallState::Failed { .. }
        )
    });
    if let Some(state) = state {
        actor.browser_install.latest = Some(state.clone());
        super::app::publish(actor, &HostEvent::BrowserInstall { state });
    }
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
            && let Err(error) = begin(actor, false)
        {
            tracing::warn!(
                error = %crate::log_file::Cause(&error),
                "the download asked for after a cancel did not start"
            );
        }
    }
}

/// The app catch-up's part: the latest state, whole, when there is one.
pub(super) fn current(actor: &Actor) -> Option<HostEvent> {
    let state = actor.browser_install.latest.clone()?;
    Some(HostEvent::BrowserInstall { state })
}

/// Shutdown: the running download is cancelled, no restart follows it, and the timer stops.
pub(super) fn shutdown(actor: &mut Actor) {
    actor.browser_install.restart = false;
    actor.browser_install.timer = None;
    if let Some(running) = &actor.browser_install.running {
        running.cancel.cancel();
    }
}

/// Whether a download still runs: shutdown waits for its last state.
pub(super) fn busy(actor: &Actor) -> bool {
    actor.browser_install.running.is_some()
}
