//! The launch's jail (decision record 0051, R17 PR-B2). With the launchers seam
//! (`Sandbox::with_launchers`), once a launch's checks have passed on the actor
//! ([`super::on_prepared`]), its policy is built ([`launch_policy`]: `build` walks the matter)
//! and the seam's `launcher` is called, both on a `clauseo-codex-fs` thread
//! ([`off_actor_or_undo`]) waited on for [`LAUNCH_WAIT`] on the host clock. The work that waits
//! for the agent stays held on the thread's `preparing` meanwhile, so the host keeps answering.
//! The answer comes back as [`TurnInput::Sandboxed`]; the agent then spawns once, through the
//! launcher the seam gave (one launcher per agent launch: its policy is fixed).
//!
//! A refused policy, an error from the seam or no answer in time fails the launch with
//! `agent.launch_failed` (a shortcut where a protected name should be keeps
//! `matter.protected_link`), its message naming the kind of failure and never a path; the run
//! folder is removed on that thread before the answer goes out. An answer nobody waits for any
//! more (an earlier launch, the actor gone) is taken back on its thread, and during shutdown on
//! the actor's removal path, so it spawns nothing.
//!
//! The names the seam set aside before the launch (Windows' `before_launch`) give one `Notice`
//! on the thread, relative to the matter. Once the agent spawned, its launcher and the seam ride
//! on the thread's run folder ([`SandboxSession`]); when the agent has exited and the run folder
//! goes, the seam's `after_session` is called once, off the actor, bounded like the removal.
//!
//! The policy is B2's minimum: the matter folder read-write, the session's `run/<session>/tmp`
//! as `TMPDIR`, the agent's install folder read and run. R17 PR-C adds the per-agent state
//! folders (until then the state folder is the session's `work` folder); PR-D the CA files,
//! skills, the browser sidecar and the login files.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use clauseo_process::{Launcher, SpawnOptions};
use clauseo_protocol::{ErrorCode, MatterId, Notice, NoticeLevel, StreamId, ThreadId};
use clauseo_sandbox::{Denial, DenialSink, Policy, SetAside};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::agents::{self, Waiting};
use super::super::out::Out;
use super::super::{Then, TurnInput, host_stopping, state};
use super::{Prepared, Preparing, RunFolders, discard, undo_late};
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, off_actor, off_actor_or_undo};
use crate::config::{SandboxLaunchers, SessionKey};
use crate::error::Error;
use crate::log_file::Cause;

/// The most set-aside names a notice lists; the rest are counted.
const NAMED: usize = 10;

/// What the seam gave a launch, carried on its [`Prepared`] to the spawn.
pub(crate) struct Sandboxed {
    /// The launch's own launcher: the agent spawns through it once.
    pub(crate) launcher: Arc<dyn Launcher>,
    /// The names set aside before the launch, relative to the matter.
    moved: Vec<String>,
}

impl fmt::Debug for Sandboxed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandboxed")
            .field("moved", &self.moved.len())
            .finish_non_exhaustive()
    }
}

/// A spawned agent's jail, on its thread's run folder until the agent has exited: the seam, for
/// `after_session`, and the launcher, kept alive for exactly that launch (a Seatbelt launcher
/// owns its denial watcher registration).
#[derive(Clone)]
pub(crate) struct SandboxSession {
    launchers: Arc<dyn SandboxLaunchers>,
    launcher: Arc<dyn Launcher>,
}

impl fmt::Debug for SandboxSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxSession").finish_non_exhaustive()
    }
}

/// The policy of one agent launch: `matter` (the folder the thread's matter row stores, as the
/// agent runs in it, `agent::session_folder`) read-write, the session's `run/<session>/tmp`
/// as its `TMPDIR` (never the run folder itself), and the agent's `install` folder read and
/// run. The state folder is the session's `work` folder until R17 PR-C's per-agent state
/// folders. R17 PR-C and PR-D extend this function. File system calls (`build` walks the
/// matter): off the actor.
pub(crate) fn launch_policy(
    matter: &Path,
    run: &RunFolders,
    install: &Path,
) -> Result<Policy, clauseo_sandbox::Error> {
    Policy::builder(crate::agent::session_folder(matter), &run.work)
        .tmpdir(run.root.join("tmp"))
        .runtime(install)
        .build()
}

/// The seam's job: a launch's policy and launcher, waited on as long as a launch's checks.
const fn job(matter: MatterId) -> Job {
    Job {
        what: "the sandbox's launcher",
        matter: Some(matter),
        wait: LAUNCH_WAIT,
    }
}

/// After a launch's checks (`id`) passed: holds `waiting` on the thread again and asks the
/// seam for the launch's launcher off the actor; [`on_sandboxed`] takes the answer.
pub(super) fn start(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    waiting: Vec<Waiting>,
    prepared: Box<Prepared>,
    seam: (Arc<dyn SandboxLaunchers>, SpawnOptions),
) {
    let (launchers, opts) = seam;
    let install = match install_folder(actor, thread) {
        Ok(install) => install,
        Err(error) => {
            discard(actor, thread, prepared.run, None);
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let Some(state) = state(actor, thread) else {
        discard(actor, thread, prepared.run, None);
        return;
    };
    state.preparing = Some(Box::new(Preparing { id, waiting }));
    let matter = prepared.run.key.matter;
    let clock = Arc::clone(&actor.clock);
    let denials: Arc<dyn DenialSink> = Arc::new(LoggedDenials { matter });
    actor.spawn_post(async move {
        let answered = off_actor_or_undo(
            clock,
            job(matter),
            move || Ok(make(&*launchers, prepared, &install, opts, denials)),
            |late: Result<Box<Prepared>, Error>| {
                if let Ok(late) = late {
                    undo_late(*late);
                }
            },
        )
        .await;
        // The thread did not answer in time, did not start or panicked (logged there).
        let result = answered.unwrap_or_else(|_unanswered| Err(unanswered()));
        Input::Turn(TurnInput::Sandboxed { thread, id, result })
    });
}

/// The agent's install folder: the folder of its program (`command`, an absolute path).
fn install_folder(actor: &Actor, thread: ThreadId) -> Result<PathBuf, Error> {
    let row = &actor.check_thread(thread)?.row;
    let settings = actor.effective.agent(row.agent.as_str()).ok_or_else(|| {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("no agent {} in the settings", row.agent),
        )
    })?;
    Path::new(&settings.command)
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::AgentLaunchFailed,
                "The sandbox couldn't start the agent: its program has no folder.",
            )
        })
}

/// On the seam's thread: the policy, then the seam's launcher. On a failure the run folder is
/// taken back here, before the answer goes out.
fn make(
    launchers: &dyn SandboxLaunchers,
    mut prepared: Box<Prepared>,
    install: &Path,
    opts: SpawnOptions,
    denials: Arc<dyn DenialSink>,
) -> Result<Box<Prepared>, Error> {
    let matter = prepared.run.key.matter;
    let made = launch_policy(&prepared.stored, &prepared.run, install).and_then(|policy| {
        let root = policy.matter().to_owned();
        let launch = launchers.launcher(policy, &matter, opts, denials)?;
        Ok((root, launch))
    });
    match made {
        Ok((root, launch)) => {
            let moved = relative_names(&launch.set_aside, &[&root, &prepared.stored]);
            prepared.sandbox = Some(Sandboxed {
                launcher: launch.launcher,
                moved,
            });
            Ok(prepared)
        }
        Err(error) => {
            let kind = kind(&error);
            tracing::warn!(%matter, kind, "the sandbox refused the agent's launch");
            undo_late(*prepared);
            Err(refused(&error))
        }
    }
}

/// The names in `set_aside`, each relative to the first of `roots` it is under; one under none
/// keeps only its last name, so no absolute path reaches the notice.
fn relative_names(set_aside: &[SetAside], roots: &[&Path]) -> Vec<String> {
    set_aside
        .iter()
        .filter_map(|moved| {
            let from = &moved.from;
            let relative = roots
                .iter()
                .find_map(|root| from.strip_prefix(root).ok())
                .or_else(|| from.is_relative().then_some(from.as_path()));
            let name = match relative {
                Some(relative) => relative
                    .components()
                    .filter_map(|part| match part {
                        Component::Normal(name) => Some(name.to_string_lossy()),
                        Component::Prefix(_)
                        | Component::RootDir
                        | Component::CurDir
                        | Component::ParentDir => None,
                    })
                    .collect::<Vec<_>>()
                    .join("/"),
                None => from.file_name()?.to_string_lossy().into_owned(),
            };
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

/// The kind of a seam's failure, for the message and the log: never its path.
fn kind(error: &clauseo_sandbox::Error) -> &'static str {
    use clauseo_sandbox::Error as E;
    match error {
        E::Policy(_) => "its rules for this matter were refused",
        E::Probe(_) => "a look at the matter failed",
        E::Launch(_) => "the jailed program did not start",
        E::ProxyConfig(_) | E::ProxyStart(_) => "the network filter did not start",
        E::FolderUnsupported { .. } | E::NeighbourUnfenceable { .. } => {
            "a folder can't be protected"
        }
        E::AclApply { .. } => "the folder permissions couldn't be set",
        E::SetAside { .. } => "a changed protected item couldn't be set aside",
        E::CleanClose(_) => "the record of the matter's last session couldn't be removed",
        E::ProtectedLink { .. } => "a protected item in the matter is a shortcut",
        E::NetworkUnfiltered { .. } => "the matter's network filter isn't in place",
        E::Blocked(_) => "it is blocked on this computer, and whoever looks after it can lift that",
    }
}

/// The launch's failure for a seam's error: `agent.launch_failed`, or `matter.protected_link`
/// for a shortcut where a protected name should be; the message names the kind only.
fn refused(error: &clauseo_sandbox::Error) -> Error {
    let code = match error {
        clauseo_sandbox::Error::ProtectedLink { .. } => ErrorCode::MatterProtectedLink,
        clauseo_sandbox::Error::Policy(_)
        | clauseo_sandbox::Error::Probe(_)
        | clauseo_sandbox::Error::Launch(_)
        | clauseo_sandbox::Error::ProxyConfig(_)
        | clauseo_sandbox::Error::ProxyStart(_)
        | clauseo_sandbox::Error::FolderUnsupported { .. }
        | clauseo_sandbox::Error::NeighbourUnfenceable { .. }
        | clauseo_sandbox::Error::AclApply { .. }
        | clauseo_sandbox::Error::SetAside { .. }
        | clauseo_sandbox::Error::CleanClose(_)
        | clauseo_sandbox::Error::NetworkUnfiltered { .. }
        | clauseo_sandbox::Error::Blocked(_) => ErrorCode::AgentLaunchFailed,
    };
    Error::refused(
        code,
        format!("The sandbox couldn't start the agent: {}.", kind(error)),
    )
}

/// The failure when the seam's thread did not answer in time (or did not start, or panicked).
fn unanswered() -> Error {
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        "The sandbox didn't prepare the agent's launch in time.",
    )
}

/// The seam answered the launch `id`. The agent spawns through its launcher, or the waiting
/// work fails with its error; during shutdown it fails as closed. On every way that does not
/// end in a spawned agent the run folder is removed off the actor (on a failure, already by
/// the seam's thread). The names it set aside give their notice whatever follows, as they
/// were moved.
pub(crate) fn on_sandboxed(
    actor: &mut Actor,
    thread: ThreadId,
    id: u64,
    result: Result<Box<Prepared>, Error>,
) {
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        if let Ok(stale) = result {
            discard(actor, thread, stale.run, None);
        }
        return;
    };
    let waiting = preparing.waiting;
    let mut prepared = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    let moved = prepared
        .sandbox
        .as_mut()
        .map(|sandboxed| std::mem::take(&mut sandboxed.moved))
        .unwrap_or_default();
    if host_stopping(actor) {
        if !moved.is_empty() {
            let moved = moved.len();
            tracing::info!(%thread, moved, "the sandbox set names aside; the host stops");
        }
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    if !moved.is_empty() {
        log_notice(actor, thread, &moved);
    }
    if let Some(error) = super::super::sign_in::launch_refusal(actor, thread) {
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &error);
        return;
    }
    let session = actor
        .protection
        .launchers()
        .zip(prepared.sandbox.as_ref())
        .map(|((launchers, _), sandboxed)| SandboxSession {
            launchers,
            launcher: Arc::clone(&sandboxed.launcher),
        });
    super::launch_prepared(actor, thread, waiting, prepared, session);
}

/// The notice of the names `moved` (relative to the matter), logged on the thread once its
/// stream is loaded: at most [`NAMED`] named, the rest counted.
fn log_notice(actor: &mut Actor, thread: ThreadId, moved: &[String]) {
    tracing::info!(%thread, moved = moved.len(), "the sandbox set protected names aside");
    let notice = notice(moved);
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| {
                let mut out = Out::new(actor, thread, None)?;
                out.push(actor, notice)?;
                out.send(actor, Then::Nothing)
            });
            if let Err(error) = logged {
                let error = Cause(&error);
                tracing::warn!(%thread, %error, "a set-aside notice was not logged");
            }
            actor.release(stream);
        }),
    );
}

/// The set-aside notice's text and level. No `NoticeKind` yet (that is a wire change, R17
/// PR-D/E): the text is the notice.
fn notice(moved: &[String]) -> Notice {
    let named = moved
        .iter()
        .take(NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let more = moved.len().saturating_sub(NAMED);
    let what = match moved.len() {
        1 => "a protected item that changed".to_owned(),
        n => format!("{n} protected items that changed"),
    };
    let rest = if more == 0 {
        String::new()
    } else {
        format!(" and {more} more")
    };
    Notice {
        level: NoticeLevel::Warning,
        text: format!(
            "Before the agent started, Clauseo moved {what} in this matter to a safe place: \
             {named}{rest}."
        ),
        kind: None,
    }
}

/// `run`'s agent has exited: the seam's `after_session` for its matter, once, off the actor,
/// waited on as a run folder's removal is (it counts as one for shutdown), and the launch's
/// launcher dropped there after it.
pub(super) fn after_session(
    actor: &mut Actor,
    thread: ThreadId,
    key: SessionKey,
    session: SandboxSession,
) {
    let matter = key.matter;
    let clock = Arc::clone(&actor.clock);
    let job = Job::short("the sandbox's after-session", Some(matter));
    actor.work.run_removals += 1;
    actor.spawn_post(async move {
        let done = off_actor(clock, job, move || {
            let SandboxSession {
                launchers,
                launcher,
            } = session;
            launchers.after_session(&matter);
            drop(launcher);
            Ok(())
        })
        .await;
        if let Err(error) = done {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "the sandbox's after-session call did not answer"
            );
        }
        // Freed nothing: the run folder's own removal frees the run.
        Input::Turn(TurnInput::RunRemoved { key, freed: false })
    });
}

/// The launch's denial sink until R18's: each denial logged by its matter only, never its path
/// or host. The launcher caps the rate in front of it.
struct LoggedDenials {
    matter: MatterId,
}

impl DenialSink for LoggedDenials {
    fn denied(&self, d: Denial) {
        drop(d);
        tracing::debug!(matter = %self.matter, "the sandbox refused something the agent asked for");
    }
}

#[cfg(test)]
mod tests;
