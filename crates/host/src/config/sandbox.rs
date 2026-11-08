//! The sandbox seam (decision record 0051, R17 PR-B1 and PR-B2): how the host learns this
//! computer's protection, runs the one-time set-up and (B2) starts each agent in the jail.
//! Without it, the computer is `Unsupported { why: not_built }` and the two protection
//! commands are `command.invalid`; the host never calls `clauseo_sandbox::detect` on its own,
//! since on macOS that would report tier A while nothing jails the agent.

use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::{Launcher, SpawnOptions};
use clauseo_protocol::{AgentId, MatterId};
use clauseo_sandbox::{DenialSink, Detection, Policy, SandboxSetup, SetAside};

use super::SessionKey;

/// The detection the host calls, off its actor: at start, and after each set-up, removal or
/// check. Desktop passes `move || clauseo_sandbox::detect(setup.as_deref())`.
pub type Detect = Arc<dyn Fn() -> Detection + Send + Sync>;

/// The install folder of an agent, by its id, from its install record (`Installed::folder` in
/// `clauseo_agents`), or `None` for an agent with none (one the lawyer installed). Called on
/// the host's actor at each jailed launch: it must do no I/O (desktop answers from the agents
/// worker's memory).
pub type InstallFolders = Arc<dyn Fn(&AgentId) -> Option<PathBuf> + Send + Sync>;

/// The OS's trusted roots, each a DER certificate, for the jail's CA bundle (R17 PR-D). Called
/// off the actor at each jailed launch. Default: `rustls-native-certs`' load, the set Node's
/// `NODE_USE_SYSTEM_CA` and Codex read outside the jail.
pub type CaRoots = Arc<dyn Fn() -> Vec<Vec<u8>> + Send + Sync>;

/// The launchers seam (R17 PR-B2): one jailed launcher per agent launch, made off the host's
/// actor. Desktop builds `SeatbeltLauncher::new(policy, opts, denials)` on macOS and tier B's
/// launcher on Windows, whose impl first calls `windows::before_launch` and returns what it
/// moved (R17 PR-D/E).
///
/// The host calls [`SandboxLaunchers::reserve`] and then [`SandboxLaunchers::launcher`] on one
/// thread of its own, bounded, after the launch's checks: the reservation first, so its ports go
/// into the launch's policy, which is built between the two calls.
pub trait SandboxLaunchers: Send + Sync {
    /// Reserves what the launch `session` of `matter` needs before its policy is built: on
    /// Windows' tier B its pool slot, user and ports; elsewhere nothing
    /// ([`Reservation::unpooled`]). `running` is the matter's other sessions that may run an
    /// agent when the call was made: launched and not yet ended, or with their own launch still
    /// in its checks or at this seam (sorted; taken on the host's actor), so Windows'
    /// `before_launch` can drop crashed sessions from the open set and add this one. The
    /// reservation is released when dropped, whatever happens to the launch after.
    ///
    /// # Errors
    ///
    /// No slot could be reserved (a full pool, a pool another matter owns).
    fn reserve(
        &self,
        matter: &MatterId,
        session: &SessionKey,
        running: &[SessionKey],
    ) -> Result<Reservation, clauseo_sandbox::Error>;

    /// The launcher for the agent launch `session` of `matter`, its policy fixed (a launcher's
    /// policy never widens, `clauseo_process::Launcher`), starting the agent with `opts`; its
    /// denials go to `denials`. `reservation` is what [`SandboxLaunchers::reserve`] gave this
    /// launch, taken by value (the launcher keeps it while it needs it); an adapter refuses one
    /// for another matter or session ([`Reservation::check`]). The host spawns the agent through
    /// the returned launcher once, keeping it until the agent's process tree has exited. An error
    /// fails the launch with the error's `code()`, its message naming the kind of failure and
    /// never a path.
    ///
    /// # Errors
    ///
    /// Whatever stops the jail: a refused policy, a reservation of another launch, a failed
    /// set-aside, a launcher that did not start.
    fn launcher(
        &self,
        policy: Policy,
        matter: &MatterId,
        session: &SessionKey,
        reservation: Reservation,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
    ) -> Result<SandboxLaunch, clauseo_sandbox::Error>;

    /// The agent of a launch this seam's [`SandboxLaunchers::launcher`] made has exited: called
    /// once per agent that spawned (never for a launch that failed before), on a thread of its
    /// own, bounded by the run folder removal's wait, with the launch's own policy. What it
    /// returns was moved out of the matter after the session (absolute `from`, as
    /// `before_launch`'s): the thread gets one notice naming it, matter-relative. Desktop's
    /// Windows impl calls `windows::after_session` here (R17 PR-D/E).
    fn after_session(&self, end: SessionEnd) -> Vec<SetAside>;
}

/// The session [`SandboxLaunchers::after_session`] closes: its matter, its key and the policy
/// its launch was given.
#[derive(Clone)]
#[non_exhaustive]
pub struct SessionEnd {
    /// The session's matter.
    pub matter: MatterId,
    /// The session.
    pub session: SessionKey,
    /// The policy the session's launcher was made with.
    pub policy: Policy,
}

impl fmt::Debug for SessionEnd {
    /// The matter and the session only: the policy holds the computer's paths.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionEnd")
            .field("matter", &self.matter)
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl SessionEnd {
    /// The end of `session` of `matter`, launched under `policy`.
    #[must_use]
    pub fn new(matter: MatterId, session: SessionKey, policy: Policy) -> Self {
        Self {
            matter,
            session,
            policy,
        }
    }
}

/// What [`SandboxLaunchers::reserve`] gave one launch: its matter and session, the loopback ports
/// its proxy listeners take (in its policy's `proxy_ports`), and on Windows' tier B its pool slot
/// and user. Released when dropped (its guard drops with it); `Send`, never `Clone`.
///
/// The host's stand-in for stage C's `clauseo_sandbox::Reservation` (#799, not on this base),
/// with the same getters: R17 PR-D3b-win swaps it for that type.
#[non_exhaustive]
pub struct Reservation {
    matter: MatterId,
    session: SessionKey,
    ports: Vec<u16>,
    slot: Option<u8>,
    pool_user: Option<Box<dyn Any + Send>>,
    /// Released when the reservation drops.
    guard: Option<Box<dyn Any + Send>>,
}

impl Reservation {
    /// No slot, no pool user, no ports: macOS and tier C, where the host binds its listeners on
    /// ports the OS picks.
    #[must_use]
    pub fn unpooled(matter: MatterId, session: SessionKey) -> Self {
        Self {
            matter,
            session,
            ports: Vec::new(),
            slot: None,
            pool_user: None,
            guard: None,
        }
    }

    /// A reservation of `ports` with no slot, for test launchers.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn fixed(ports: Vec<u16>, matter: MatterId, session: SessionKey) -> Self {
        Self {
            ports,
            ..Self::unpooled(matter, session)
        }
    }

    /// This reservation, released by dropping `guard` when it drops.
    #[must_use]
    pub fn with_guard(mut self, guard: Box<dyn Any + Send>) -> Self {
        self.guard = Some(guard);
        self
    }

    /// The matter it was reserved for.
    #[must_use]
    pub fn matter(&self) -> &MatterId {
        &self.matter
    }

    /// The launch it was reserved for.
    #[must_use]
    pub fn session(&self) -> &SessionKey {
        &self.session
    }

    /// The loopback ports its listeners take; empty where the OS picks them.
    #[must_use]
    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    /// The pool slot, on Windows' tier B.
    #[must_use]
    pub fn slot(&self) -> Option<u8> {
        self.slot
    }

    /// The pool user, opaque to the host, on Windows' tier B.
    #[must_use]
    pub fn pool_user(&self) -> Option<&(dyn Any + Send)> {
        self.pool_user.as_deref()
    }

    /// Refuses a reservation made for another matter or launch than `matter` and `session`.
    ///
    /// # Errors
    ///
    /// `clauseo_sandbox::Error::Policy`, naming neither path nor id.
    pub fn check(
        &self,
        matter: &MatterId,
        session: &SessionKey,
    ) -> Result<(), clauseo_sandbox::Error> {
        if self.matter == *matter && self.session == *session {
            Ok(())
        } else {
            Err(clauseo_sandbox::Error::Policy(
                "the reservation is another launch's".to_owned(),
            ))
        }
    }
}

impl fmt::Debug for Reservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reservation")
            .field("matter", &self.matter)
            .field("session", &self.session)
            .field("ports", &self.ports)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
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
    /// The agents' install folders, read-only in their jail (R17 PR-D); `None`: only the
    /// folder of each agent's program.
    pub install_folders: Option<InstallFolders>,
    /// The OS's roots for the jail's CA bundle (R17 PR-D); `None`: `rustls-native-certs`'.
    pub ca_roots: Option<CaRoots>,
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
            install_folders: None,
            ca_roots: None,
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

    /// Each agent's install folder from its install record ([`InstallFolders`]), granted
    /// read-only and run in its jail beside its program's folder.
    #[must_use]
    pub fn with_install_folders(mut self, folders: InstallFolders) -> Self {
        self.install_folders = Some(folders);
        self
    }

    /// The OS roots the jail's CA bundle takes ([`CaRoots`]), in place of
    /// `rustls-native-certs`' (tests pass fixed ones).
    #[must_use]
    pub fn with_ca_roots(mut self, roots: CaRoots) -> Self {
        self.ca_roots = Some(roots);
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
