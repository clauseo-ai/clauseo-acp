//! The sandbox seam (decision record 0051, R17 PR-B1 and PR-B2): how the host learns this
//! computer's protection, runs the one-time set-up and (B2) starts each agent in the jail.
//! Without it, the computer is `Unsupported { why: not_built }` and the two protection
//! commands are `command.invalid`; the host never calls `clauseo_sandbox::detect` on its own,
//! since on macOS that would report tier A while nothing jails the agent.

use std::fmt;
use std::sync::Arc;

use clauseo_process::{Launcher, SpawnOptions};
use clauseo_protocol::MatterId;
use clauseo_sandbox::{DenialSink, Detection, Policy, SandboxSetup, SetAside};

/// The detection the host calls, off its actor: at start, and after each set-up, removal or
/// check. Desktop passes `move || clauseo_sandbox::detect(setup.as_deref())`.
pub type Detect = Arc<dyn Fn() -> Detection + Send + Sync>;

/// The launchers seam (R17 PR-B2): one jailed launcher per agent launch, made off the host's
/// actor. Desktop builds `SeatbeltLauncher::new(policy, opts, denials)` on macOS and tier B's
/// launcher on Windows, whose impl first calls `windows::before_launch` and returns what it
/// moved (R17 PR-D/E).
pub trait SandboxLaunchers: Send + Sync {
    /// The launcher for one agent launch of `matter`, its policy fixed (a launcher's policy never
    /// widens, `clauseo_process::Launcher`), starting the agent with `opts`; its denials go to
    /// `denials`. The host calls it on a thread of its own, bounded, after the launch's checks,
    /// and spawns the agent through the returned launcher once, keeping it until the agent's
    /// process tree has exited. An error fails the launch (`agent.launch_failed`), its message
    /// naming the kind of failure and never a path.
    ///
    /// # Errors
    ///
    /// Whatever stops the jail: a refused policy, a failed set-aside, a launcher that did not
    /// start.
    fn launcher(
        &self,
        policy: Policy,
        matter: &MatterId,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
    ) -> Result<SandboxLaunch, clauseo_sandbox::Error>;

    /// The agent of a launch this seam's [`SandboxLaunchers::launcher`] made has exited: called
    /// once per agent that spawned (never for a launch that failed before), on a thread of its
    /// own, bounded by the run folder removal's wait. Desktop's Windows impl calls
    /// `windows::after_session` here (R17 PR-D/E).
    fn after_session(&self, matter: &MatterId);
}

/// What [`SandboxLaunchers::launcher`] gives: the launch's own launcher and the protected names
/// it set aside before the launch, for the thread's notice.
#[non_exhaustive]
pub struct SandboxLaunch {
    /// The launcher the agent spawns through, once.
    pub launcher: Arc<dyn Launcher>,
    /// What was moved out of the matter before the launch (Windows' `before_launch`); empty
    /// elsewhere.
    pub set_aside: Vec<SetAside>,
}

impl SandboxLaunch {
    /// A launch through `launcher`, having set aside `set_aside`.
    #[must_use]
    pub fn new(launcher: Arc<dyn Launcher>, set_aside: Vec<SetAside>) -> Self {
        Self {
            launcher,
            set_aside,
        }
    }
}

impl fmt::Debug for SandboxLaunch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxLaunch")
            .field("set_aside", &self.set_aside.len())
            .finish_non_exhaustive()
    }
}

/// What the host holds of the sandbox: its detection and, where the OS has one (Windows' tier
/// B), the set-up; with [`Sandbox::with_launchers`] (R17 PR-B2) the launchers every agent
/// launch goes through. Built with [`Sandbox::new`]; later steps add fields, so it is
/// non-exhaustive.
#[derive(Clone)]
#[non_exhaustive]
pub struct Sandbox {
    /// This computer's tier, and why it is C.
    pub detect: Detect,
    /// The one-time set-up, where the OS has one; `None` on macOS and Linux.
    pub setup: Option<Arc<dyn SandboxSetup>>,
    /// The launchers (R17 PR-B2): with them every agent launch goes through the jail; without
    /// them, through the host's launcher (`HostConfig::new`), as before.
    pub launchers: Option<Arc<dyn SandboxLaunchers>>,
    /// The options each jailed launch starts the agent with, passed to
    /// [`SandboxLaunchers::launcher`].
    pub launch_opts: SpawnOptions,
    /// The set-up thread fails to start, as the OS can refuse a thread (tests only; set it with
    /// [`Sandbox::failing_spawn`]).
    #[cfg(feature = "test-support")]
    pub fail_spawn: bool,
}

impl Sandbox {
    /// The seam from its detection and set-up.
    #[must_use]
    pub fn new(detect: Detect, setup: Option<Arc<dyn SandboxSetup>>) -> Self {
        Self {
            detect,
            setup,
            launchers: None,
            launch_opts: SpawnOptions::new(),
            #[cfg(feature = "test-support")]
            fail_spawn: false,
        }
    }

    /// The launchers every agent launch goes through (R17 PR-B2), each launch's agent started
    /// with `opts` (desktop passes its host's spawn options).
    #[must_use]
    pub fn with_launchers(
        mut self,
        launchers: Arc<dyn SandboxLaunchers>,
        opts: SpawnOptions,
    ) -> Self {
        self.launchers = Some(launchers);
        self.launch_opts = opts;
        self
    }

    /// The set-up thread fails to start, as the OS can refuse a thread, for tests of the answer.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn failing_spawn(mut self) -> Self {
        self.fail_spawn = true;
        self
    }
}

impl fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandbox")
            .field("setup", &self.setup.is_some())
            .field("launchers", &self.launchers.is_some())
            .finish_non_exhaustive()
    }
}
