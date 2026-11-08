//! The launcher `Launchers::launch` returns: one agent each, started through the outer runner.
//!
//! - **The outer:** the launcher makes the denial pipe and writes the protected launch file,
//!   which names it, then starts
//!   `clauseo-sandbox-run.exe --launch <file>` in the host's tree ([`outer_spec`]), with a
//!   minimal environment ([`OUTER_ENV`]) and `clear_env`: the agent's own environment travels
//!   in the launch file. The outer runs as the lawyer and makes the agent's window station and
//!   desktop itself (#786), so the launcher does nothing for desktops.
//! - **Its options** ([`outer_options`]): the host's, with `clear_env` and a 5 s stdin grace
//!   unless the host set one. Nothing is read from the outer's stderr: it carries the agent's,
//!   so nothing there is trusted; the host only drains it.
//! - **Denials:** once the outer has started, a thread of its own reads the denial pipe into
//!   the launch's sink, from the outer alone (`feed.rs`).
//! - **A cancel** is the host's shutdown ladder on the outer's handle: stdin closed (the outer
//!   passes the close on to the agent), the grace, then the tree's kill (`clauseo_process`).
//! - **The claim's hand-over** (C3): `Launchers::launch` gives this launcher the reservation's
//!   claim, armed. It is let go of only once the outer has started. If the pipe, the launch
//!   file or the start fails, the session is undone here before the error returns (the run's
//!   group, the slot, the session's mark) and the matter freed; if the launcher is dropped
//!   unused, the claim's drop does the same, off the dropping thread. No pass runs then.
//! - **Before it starts anything** the launcher checks its session is still launched in the
//!   registry (an `after_session` may have ended it): if not, `WrongReservation`. **A panic in
//!   the start is caught** (C3): the outer it may have started is ended first (its job killed
//!   and seen truly empty, `LaunchSteps::end_outer`), and only then is the claim released; if
//!   the outer can't be ended, the matter is marked stuck, never released.
//! - **The end:** once the outer started, the host decodes its exit ([`RunEnd`](super::RunEnd)),
//!   then the session's after-session pass runs once (`end.rs`); dropping the launcher runs it
//!   if nobody has.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions};

use super::end::SessionGuard;
use super::feed::{DenialPipe, feed};
use super::registry::Registry;
use super::steps::LaunchSteps;
use crate::reservation::Claim;
use crate::windows::pool::PoolUser;
use crate::windows::runner::runner_args;
use crate::{DenialSink, Error, Policy};

/// How long the host's shutdown waits after closing the outer's stdin, unless the host set its
/// own: the outer passes the close to the agent and waits for it, then for its own end.
pub const OUTER_STDIN_GRACE: Duration = Duration::from_secs(5);

/// The host's variables the outer gets, when set; nothing else. Windows' own DLLs look for the
/// system folder in the first three; the temporary folder is the lawyer's, the outer's user.
pub const OUTER_ENV: [&str; 5] = ["SystemRoot", "SystemDrive", "windir", "TEMP", "TMP"];

/// The outer's options: the host's `opts`, with `clear_env` and [`OUTER_STDIN_GRACE`] unless
/// the host set a grace.
pub(super) fn outer_options(opts: SpawnOptions) -> SpawnOptions {
    let grace = opts.stdin_grace_set().unwrap_or(OUTER_STDIN_GRACE);
    opts.clear_env().stdin_grace(grace)
}

/// The outer's spec: `runner` with `--launch <launch>`, in the runner's folder, with the
/// [`OUTER_ENV`] variables `var` gives.
pub(super) fn outer_spec(
    runner: PathBuf,
    launch: &Path,
    var: impl Fn(&str) -> Option<OsString>,
) -> Result<LaunchSpec, clauseo_process::Error> {
    let folder = runner.parent().map(Path::to_path_buf).ok_or_else(|| {
        clauseo_process::Error::InvalidSpec("the runner has no folder".to_owned())
    })?;
    let env: BTreeMap<OsString, OsString> = OUTER_ENV
        .into_iter()
        .filter_map(|name| var(name).map(|value| (OsString::from(name), value)))
        .collect();
    LaunchSpec::new(runner, runner_args(launch), env, folder)
}

/// Starts one agent as the matter's pool user: makes the denial pipe, writes the protected
/// launch file, then starts `clauseo-sandbox-run.exe --launch <file>` in the host's tree. Holds
/// the session's claim until the outer starts, then its end (`end.rs`) until dropped.
pub struct TierBLauncher<S: LaunchSteps> {
    steps: Arc<S>,
    /// The session's claim, until its outer starts: released (the session undone) if it never
    /// does.
    claim: Mutex<Option<Claim>>,
    pool: PoolUser,
    opts: SpawnOptions,
    denials: Arc<dyn DenialSink>,
    started: AtomicBool,
    guard: SessionGuard<S>,
}

/// What the launcher of a launched session is made from.
pub(super) struct Handover {
    pub(super) matter: String,
    pub(super) session: String,
    pub(super) claim: Claim,
    pub(super) policy: Policy,
    pub(super) pool: PoolUser,
}

impl<S: LaunchSteps> TierBLauncher<S> {
    /// The launcher of the launched `session` in `registry`; `opts` are the outer's
    /// ([`outer_options`]), `denials` the launch's sink.
    pub(super) fn new(
        registry: Arc<Registry<S>>,
        session: Handover,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
    ) -> TierBLauncher<S> {
        let Handover {
            matter,
            session,
            claim,
            policy,
            pool,
        } = session;
        TierBLauncher {
            steps: Arc::clone(&registry.steps),
            guard: SessionGuard {
                id: claim.id,
                registry,
                matter,
                session,
                policy,
                armed: AtomicBool::new(false),
            },
            claim: Mutex::new(Some(claim)),
            pool,
            opts,
            denials,
            started: AtomicBool::new(false),
        }
    }

    fn policy(&self) -> &Policy {
        &self.guard.policy
    }

    /// Makes the pipe, writes the launch file and starts the outer; the feed starts with it.
    fn start(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let (pipe, pipe_name) = self.steps.create_pipe().map_err(launch_error)?;
        let file = self
            .steps
            .write_launch(self.policy(), &self.pool, spec, &pipe_name)
            .map_err(launch_error)?;
        let outer = outer_spec(self.steps.runner(), &file, |name| self.steps.host_var(name));
        let started = outer.and_then(|outer| self.steps.spawn_outer(&outer, &self.opts));
        match &started {
            Ok(launched) => {
                self.guard.started();
                start_feed(pipe, launched.handle.pid(), Arc::clone(&self.denials));
            }
            Err(_) => self.steps.remove_launch(&file),
        }
        started
    }
}

impl<S: LaunchSteps> Launcher for TierBLauncher<S> {
    /// Starts the agent `spec` names; a second call is refused: each launcher starts one agent.
    /// A launch file whose outer didn't start is removed, its pipe closed, and the session
    /// undone before the error returns.
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(clauseo_process::Error::InvalidSpec(
                "a tier B launcher starts one agent".to_owned(),
            ));
        }
        let registry = &self.guard.registry;
        let claim = self
            .claim
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(claim) = claim.filter(|claim| registry.is_launched(claim.id)) else {
            // Its end began (or it was released): nothing is started for a session gone.
            return Err(clauseo_process::Error::Os {
                what: "the sandbox's reservation is no longer held",
                source: std::io::Error::other(Error::WrongReservation),
            });
        };
        let Ok(started) = std::panic::catch_unwind(AssertUnwindSafe(|| self.start(spec))) else {
            tracing::warn!("tier B's launcher panicked while starting the outer");
            let id = claim.id;
            claim.disarm();
            match self.steps.end_outer(&self.pool) {
                Ok(()) => registry.release_now(id),
                Err(error) => {
                    tracing::warn!(kind = error.kind(), "tier B couldn't end the outer");
                    registry.stick(id);
                }
            }
            return Err(clauseo_process::Error::Os {
                what: "the sandbox's launcher failed",
                source: std::io::Error::other("a step panicked"),
            });
        };
        hand_over(registry, Some(claim), started.is_ok());
        started
    }
}

/// Hands `claim` over once the start is known: started, it is let go of (`after_session` ends
/// the session from here); not started, the session is undone and the matter freed before the
/// error returns.
pub(super) fn hand_over<S: LaunchSteps>(
    registry: &Registry<S>,
    claim: Option<Claim>,
    started: bool,
) {
    if let Some(claim) = claim {
        let id = claim.id;
        claim.disarm();
        if !started {
            registry.release_now(id);
        }
    }
}

/// Starts the denial pipe's feed for the outer `pid` on a thread of its own, which ends when
/// the outer closes the pipe or exits. A thread that won't start leaves the pipe closed: the
/// session runs, with no set-aside reaching the sink.
fn start_feed<P: DenialPipe>(pipe: P, pid: u32, denials: Arc<dyn DenialSink>) {
    let started = std::thread::Builder::new()
        .name("clauseo-tierb-denials".to_owned())
        .spawn(move || feed(pipe, pid, &*denials));
    if started.is_err() {
        tracing::warn!("tier B's denial pipe thread didn't start");
    }
}

/// The sandbox's error making the pipe or writing the launch file, as the process seam's.
fn launch_error(error: Error) -> clauseo_process::Error {
    if let Error::Launch(error) = error {
        error
    } else {
        clauseo_process::Error::Os {
            what: "the sandbox couldn't prepare the outer runner's launch",
            source: std::io::Error::other(error.kind()),
        }
    }
}

impl<S: LaunchSteps> std::fmt::Debug for TierBLauncher<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TierBLauncher")
            .field("pool", &self.pool.name)
            .field("started", &self.started.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}
