//! [`SeatbeltLauncher`]: runs an agent under its policy's profile through
//! `/usr/bin/sandbox-exec`, which applies the profile and execs the agent in place, so the pid,
//! the process group and the tree kill stay `process`'s.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions, spawn_in_tree};

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
pub struct SeatbeltLauncher {
    policy: Arc<Policy>,
    profile: SeatbeltProfile,
    tag: String,
    opts: SpawnOptions,
    sink: Arc<LaunchSink>,
    registration: Mutex<Option<Registration>>,
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
    /// (the state folder's `tmp`), which `launch` starts with no inherited variables; the same
    /// working folder and the same secrets for the stderr tail to hide.
    pub fn jailed_spec(&self, spec: &LaunchSpec) -> Result<LaunchSpec, clauseo_process::Error> {
        let mut args = self.profile.args();
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
        let jailed = self.jailed_spec(spec)?;
        make_tmpdir(&self.policy.tmpdir()).map_err(|source| clauseo_process::Error::Os {
            what: "the agent's TMPDIR in its state folder could not be made",
            source,
        })?;
        self.register();
        spawn_in_tree(&jailed, &self.opts)
    }
}

/// Makes `<state>/tmp` if it is missing; the state folder exists (the policy checked it).
/// Only the folder itself is made, never through a link: an agent of an earlier launch may
/// have put a symbolic link there, and this process is outside the jail.
#[expect(
    clippy::disallowed_methods,
    reason = "the launcher's one file change: the agent's TMPDIR inside its own state folder"
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
