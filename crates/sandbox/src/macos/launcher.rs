//! [`SeatbeltLauncher`]: runs an agent under its policy's profile through
//! `/usr/bin/sandbox-exec`, which applies the profile and execs the agent in place, so the pid,
//! the process group and the tree kill stay `process`'s.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_process::{
    LaunchSpec, Launched, Launcher, SpawnOptions, TreeKill, TreeWatch, spawn_in_tree,
};

use super::git_watch::{self, Mode, Session};
use super::jail_procs::Probe;
use super::profile::{self, SeatbeltProfile};
use super::watch::{self, Registration};
use crate::{CappedSink, DenialSink, Policy};

/// The Seatbelt front end.
pub(crate) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Numbers this process's launchers, for their tags.
static NEXT_TAG: AtomicU64 = AtomicU64::new(0);

/// The capped sink one launcher's denials go through.
pub(crate) type LaunchSink = CappedSink<Arc<dyn DenialSink>>;

/// Starts agents in a Seatbelt jail. The policy, and so the profile, is fixed when the
/// launcher is made; every launch uses it.
///
/// On its first launch the launcher registers its tag, policy and sink with the host-wide
/// denial watcher, which starts `log stream` if it is the first; dropping the launcher
/// unregisters it, and the last one stops the watcher. So the host keeps the launcher alive
/// while its agent runs: denials of an agent whose launcher is gone are not reported.
///
/// Each launch starts the git watch on the matter and on the session's notes folder before the
/// agent, and it runs until every process of the session is gone, whatever happens to the
/// launcher (decision record 0038). Each launch's profile gets a probe name of its own, with a
/// random part made once per host process, by which the watch finds the session's processes
/// even after `setsid`. A launch is refused when the policy names no
/// [`git_set_aside`](crate::PolicyBuilder::git_set_aside) folder, a set-aside folder can't be
/// made, a folder's `FSEvents` stream can't start, or the matter's state refuses it (an item
/// left in place, or a process of an earlier session that outlived its kill). The host calls
/// [`git_watch_start_up`](crate::git_watch_start_up) once at its start and
/// [`git_watch_quit`](crate::git_watch_quit) at its quit, so a session a quit or crash cut off
/// still gets its final scan.
pub struct SeatbeltLauncher {
    policy: Arc<Policy>,
    profile: SeatbeltProfile,
    tag: String,
    opts: SpawnOptions,
    sink: Arc<LaunchSink>,
    registration: Mutex<Option<Registration>>,
    /// Numbers this launcher's launches, for the git watch's session ids.
    launches: AtomicU64,
}

impl std::fmt::Debug for SeatbeltLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeatbeltLauncher")
            .field("tag", &self.tag)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl SeatbeltLauncher {
    /// A launcher for `policy`, starting agents with `opts` plus [`SpawnOptions::clear_env`],
    /// always: the jailed process gets the spec's variables plus `TMPDIR` and nothing from this
    /// process's environment. Its denials reach `denials` through the rate cap
    /// ([`CappedSink`]); the watcher ticks the cap once a second.
    pub fn new(
        policy: Policy,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
    ) -> SeatbeltLauncher {
        let n = NEXT_TAG.fetch_add(1, Ordering::Relaxed);
        let tag = format!("{}{}-{n}", profile::TAG_PREFIX, std::process::id());
        let profile = profile::build(&policy, &tag);
        SeatbeltLauncher {
            policy: Arc::new(policy),
            profile,
            tag,
            opts: opts.clear_env(),
            sink: Arc::new(CappedSink::new(denials)),
            registration: Mutex::new(None),
            launches: AtomicU64::new(0),
        }
    }

    /// The profile every launch runs under.
    pub fn profile(&self) -> &SeatbeltProfile {
        &self.profile
    }

    /// The launch tag every deny rule carries: `clauseo-<pid of the host>-<counter>`.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The policy.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The spec `launch` starts for `spec`: `sandbox-exec` with the profile and its
    /// parameters, then the agent's program and arguments; the agent's variables plus `TMPDIR`
    /// ([`Policy::tmpdir`]), which `launch` starts with no inherited variables; the same
    /// working folder and the same secrets for the stderr tail to hide.
    pub fn jailed_spec(&self, spec: &LaunchSpec) -> Result<LaunchSpec, clauseo_process::Error> {
        self.jailed_spec_with(spec, &self.profile.args())
    }

    /// [`SeatbeltLauncher::jailed_spec`] with `sandbox-exec`'s arguments given.
    fn jailed_spec_with(
        &self,
        spec: &LaunchSpec,
        profile_args: &[OsString],
    ) -> Result<LaunchSpec, clauseo_process::Error> {
        let mut args = profile_args.to_vec();
        args.push(OsString::from(spec.program()));
        args.extend(spec.args().iter().cloned());
        let mut env = spec.env().clone();
        env.insert(
            OsString::from("TMPDIR"),
            OsString::from(self.policy.tmpdir()),
        );
        LaunchSpec::new(
            PathBuf::from(SANDBOX_EXEC),
            args,
            env,
            spec.cwd().to_owned(),
        )
        .map(|jailed| jailed.with_redacted(spec.redacted().iter().cloned()))
    }

    /// Registers with the watcher once, on the first launch.
    fn register(&self) {
        let mut registration = self
            .registration
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if registration.is_none() {
            *registration = Some(watch::register(
                self.tag.clone(),
                Arc::clone(&self.policy),
                Arc::clone(&self.sink),
            ));
        }
    }
}

impl Launcher for SeatbeltLauncher {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let n = self.launches.fetch_add(1, Ordering::Relaxed);
        let probe = Probe::new(&self.tag, n);
        let jailed =
            self.jailed_spec_with(spec, &profile::args_with_probe(&self.profile, probe.name()))?;
        make_tmpdir(&self.policy.tmpdir()).map_err(|source| clauseo_process::Error::Os {
            what: "the agent's TMPDIR could not be made",
            source,
        })?;
        self.register();
        let notices = Arc::clone(self.sink.inner());
        let session = format!("{}-{n}", self.tag);
        let watching = git_watch::start(&self.policy, &session, &probe, &notices, Mode::Live)
            .map_err(|source| clauseo_process::Error::Os {
                what: "the git watch could not start",
                source,
            })?;
        let launched = spawn_in_tree(&jailed, &self.opts)?;
        watching.attach(Tree {
            watch: launched.handle.tree_watch(),
            kill: launched.handle.tree_kill(),
            probe,
        });
        Ok(launched)
    }
}

/// The jailed session, for the git watch: its process group and tree, and every process
/// carrying its probe, which finds one that left the group with `setsid` and lost its parent.
struct Tree {
    watch: TreeWatch,
    kill: TreeKill,
    probe: Probe,
}

impl Session for Tree {
    fn gone(&self) -> bool {
        self.watch.gone() && self.probe.gone()
    }

    fn end(&self) {
        if !self.kill.kill() {
            tracing::error!("the jailed tree's kill did not go");
        }
        let sent = self.probe.kill_all();
        tracing::debug!(sent, "the jailed session's processes were sent SIGKILL");
    }
}

/// Makes the agent's `TMPDIR` ([`Policy::tmpdir`]) if it is missing; the folder it is in exists
/// (the policy checked it).
/// Only the folder itself is made, never through a link: an agent of an earlier launch may
/// have put a symbolic link there, and this process is outside the jail.
#[expect(
    clippy::disallowed_methods,
    reason = "the launcher's one file change: the agent's TMPDIR, its session's temp folder"
)]
fn make_tmpdir(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::other(format!(
            "{} is not a folder",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::create_dir(path),
        Err(error) => Err(error),
    }
}
