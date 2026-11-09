//! [`SeatbeltLauncher`]: runs an agent under its policy's profile through
//! `/usr/bin/sandbox-exec`, which applies the profile and execs the agent in place, so the pid,
//! the process group and the tree kill stay `process`'s. The agent leads a session of its own
//! under the process cap (`process_cap.rs`): its `RLIMIT_NPROC` and a watchdog thread. Each
//! launcher makes a session mark before its first launch (`sweep.rs`), which its profile names,
//! so every process of its launches can be found and ended wherever it went.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use clauseo_process::{LaunchSpec, Launched, Launcher, SpawnOptions, spawn_in_tree};

use super::process_cap::libproc::Libproc;
use super::process_cap::watchdog::System as _;
use super::process_cap::{self, ProcessCapSession, libproc, watchdog};
use super::profile::{self, SeatbeltProfile};
use super::sweep::{self, JailedSession, ProcId, SessionMark};
use super::watch::{self, Registration};
use crate::policy::under;
use crate::{CappedSink, DenialSink, Error, Policy};

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
    cap: ProcessCapSession,
    mark: Arc<SessionMark>,
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
    /// A launcher for `policy`, starting agents with `opts` plus [`SpawnOptions::clear_env`]
    /// and [`SpawnOptions::close_other_fds`], always: the jailed process gets the spec's
    /// variables plus `TMPDIR`, nothing from this process's environment, and no descriptor of
    /// this process's but its stdio. Its denials reach `denials` through the rate cap
    /// ([`CappedSink`]); the watcher ticks the cap once a second.
    ///
    /// `cap` is the process cap of the lawyer session this launch belongs to: its trips (the
    /// third ends the agent) and its notices, each once a session. The host makes exactly one
    /// [`ProcessCapSession`] per lawyer session and gives a clone of it to every launcher of
    /// that session; one made per launcher would reset both at every launch.
    ///
    /// The launcher's [`SessionMark`] is made here, in `marks` (the host's own folder for
    /// them, under its data dir), and its profile lets the agent write that one file's data.
    /// [`Error::Policy`] when `marks` does not exist or sits in a folder the policy lets the
    /// agent write (the matter, the state, notes, temp or browser socket folder, the Claude
    /// store, the credential link): an agent could move the mark away and blind the sweep.
    /// [`Error::Launch`] when the mark cannot be made. Before it, the stale marks of an earlier
    /// host's launchers in `marks` are removed: those no process matches any longer.
    pub fn new(
        policy: Policy,
        opts: SpawnOptions,
        denials: Arc<dyn DenialSink>,
        cap: ProcessCapSession,
        marks: &Path,
    ) -> Result<SeatbeltLauncher, Error> {
        let n = NEXT_TAG.fetch_add(1, Ordering::Relaxed);
        let tag = format!("{}{}-{n}", profile::TAG_PREFIX, std::process::id());
        let marks = check_marks(&policy, marks)?;
        // Marks a crashed earlier host left, which no process matches any longer.
        let stale = sweep::clean_stale(&marks);
        if stale > 0 {
            tracing::info!(stale, "stale session marks removed");
        }
        let mark =
            SessionMark::create(&marks, &tag).map_err(|source| clauseo_process::Error::Os {
                what: "the launch's session mark could not be made",
                source,
            })?;
        let profile = profile::build_marked(&policy, &tag, Some(mark.path()));
        Ok(SeatbeltLauncher {
            policy: Arc::new(policy),
            profile,
            tag,
            opts: opts.clear_env().close_other_fds(),
            sink: Arc::new(CappedSink::new(denials)),
            registration: Mutex::new(None),
            cap,
            mark: Arc::new(mark),
        })
    }

    /// The launcher's session mark, shared by all its launches: what the mode-switch gate and
    /// session end pass to [`sweep::switch_to_read_only`] and [`sweep::end_session`].
    pub fn session_mark(&self) -> &Arc<SessionMark> {
        &self.mark
    }

    /// [`Launcher::launch`], with the launch's session handle: the agent's tree and the
    /// launcher's mark ([`JailedSession`]).
    pub fn launch_jailed(
        &self,
        spec: &LaunchSpec,
    ) -> Result<(Launched, JailedSession), clauseo_process::Error> {
        let jailed = self.jailed_spec(spec)?;
        make_tmpdir(&self.policy.tmpdir()).map_err(|source| clauseo_process::Error::Os {
            what: "the agent's TMPDIR could not be made",
            source,
        })?;
        // A mark that is gone (removed after its session ended, or by hand) would hide every
        // process of this launch from the sweep: no launch then.
        self.mark
            .check_files()
            .map_err(|error| clauseo_process::Error::Os {
                what: "the launch's session mark is missing",
                source: io::Error::other(error),
            })?;
        let limit = launch_limit()?;
        self.register();
        let opts = self.opts.clone().own_session().max_processes(limit);
        let launched = spawn_in_tree(&jailed, &opts)?;
        let pid = launched.handle.pid();
        // Read while the agent is this process's unreaped child; an agent gone already has
        // nothing to find.
        let root = ProcId::new(pid, Libproc.info(pid).map_or(0, |agent| agent.start));
        sweep::register(&self.mark, root);
        // A watchdog that cannot start ends the agent (`launched` drops, and its tree with it):
        // it never runs without its cap.
        watchdog::watch(pid, self.cap.clone()).map_err(|source| clauseo_process::Error::Os {
            what: "the agent's process cap could not start",
            source,
        })?;
        let session =
            JailedSession::new(launched.handle.tree_watch(), Arc::clone(&self.mark), root);
        Ok((launched, session))
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
    /// [`SeatbeltLauncher::launch_jailed`] without its session handle: the launcher keeps
    /// the mark ([`SeatbeltLauncher::session_mark`]).
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.launch_jailed(spec).map(|(launched, _)| launched)
    }
}

/// The agent's `RLIMIT_NPROC` ([`process_cap::process_limit`]), from `kern.maxprocperuid`,
/// `kern.maxproc` and this process's hard limit, read at each launch.
fn launch_limit() -> Result<u64, clauseo_process::Error> {
    let read = |source| clauseo_process::Error::Os {
        what: "this Mac's process limit could not be read",
        source,
    };
    process_cap::process_limit(
        libproc::per_user().map_err(read)?,
        libproc::max_proc().map_err(read)?,
        libproc::hard_limit().map_err(read)?,
    )
}

/// `marks`, canonical, unless it does not exist or sits in a folder `policy` lets the agent
/// write.
fn check_marks(policy: &Policy, marks: &Path) -> Result<PathBuf, Error> {
    let marks = dunce::canonicalize(marks).map_err(|error| {
        Error::Policy(format!(
            "the session marks folder {}: {error}",
            marks.display()
        ))
    })?;
    let tmpdir = policy.tmpdir();
    let mut writable: Vec<&Path> = vec![policy.matter(), policy.state(), &tmpdir];
    writable.extend(policy.notes());
    writable.extend(policy.browser_socket_dir());
    writable.extend(policy.credential_link());
    let store: Vec<PathBuf> = policy
        .claude_store()
        .map(|store| [store.files(), store.lock_folders()].concat())
        .unwrap_or_default();
    writable.extend(store.iter().map(PathBuf::as_path));
    if writable.iter().any(|folder| under(&marks, folder)) {
        return Err(Error::Policy(format!(
            "the session marks folder {} is in a folder the agent may write",
            marks.display()
        )));
    }
    Ok(marks)
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
