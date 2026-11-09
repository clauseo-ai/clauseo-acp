//! The launch's jail (decision record 0051, R17 PR-B2). With the launchers seam
//! (`Sandbox::with_launchers`), once a launch's checks have passed on the actor
//! ([`super::on_prepared`]), its policy is built ([`launch_policy`]: `build` walks the matter)
//! and the seam's `launcher` is called, both on a `clauseo-codex-fs` thread
//! ([`off_actor_or_undo`]) waited on for [`LAUNCH_WAIT`] on the host clock. The work that waits
//! for the agent stays held on the thread's `preparing` meanwhile, so the host keeps answering.
//! The answer comes back as [`SandboxInput::Sandboxed`]; the agent then spawns once, through the
//! launcher the seam gave (one launcher per agent launch: its policy is fixed).
//!
//! A refused policy or an error from the seam fails the launch with the error's `code()`
//! (`agent.launch_failed`, `matter.protected_link` or `internal`), no answer in time with
//! `agent.launch_failed`, the message naming the kind of failure and never a path; the run
//! folder is removed on that thread before the answer goes out. The prep rides to that thread in
//! an [`UndoOnDrop`], so a thread that never starts or a panic there takes the run folder back
//! too. An answer nobody waits for any more (an earlier launch, the actor gone) is taken back on
//! its thread, and during shutdown on the actor's removal path, so it spawns nothing; the names
//! it set aside still get their notice.
//!
//! The seam gets the launch's `SessionKey` and those of the matter's other sessions still
//! running, taken on the actor when the job starts ([`running`]), in `reserve` first: the
//! reservation's ports go into the policy, and it is handed to `launcher` by value (dropped,
//! so released, on every failure in between). The names it set aside before
//! the launch (Windows' `before_launch`) give one `Notice` on the thread, relative to the matter
//! ([`notice`]). Once the agent spawned, its launcher, its policy and the seam ride on the
//! thread's run folder ([`SandboxSession`]); when the agent has exited and the run folder goes,
//! the seam's `after_session` is called once with that policy, off the actor, bounded like the
//! removal, and what it moved gives one more notice.
//!
//! The policy and the variables the agent gets with it (the session proxy's, the CA bundle's)
//! are R17 PR-D's ([`policy`]); the variables ride on [`Sandboxed`] to the spawn. A jailed Codex
//! starts in the jailed preset (`codex::launch_mode`); a launch in any other mode is refused
//! before the seam is asked (`codex::jailed::check_launch`).

use std::fmt;
use std::sync::Arc;

use std::ffi::OsString;

use clauseo_process::{Launcher, SpawnOptions};
use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use clauseo_sandbox::{Denial, DenialSink, Policy};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::agents::{self, Waiting};
use super::super::{TurnInput, host_stopping, state};
use super::{Prepared, Preparing, discard, undo_late};
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, off_actor, off_actor_or_undo};
use crate::config::{SandboxLaunchers, SessionEnd, SessionKey};
use crate::error::Error;
use crate::log_file::Cause;

/// What the seam gave a launch, carried on its [`Prepared`] to the spawn.
pub(crate) struct Sandboxed {
    /// The launch's own launcher: the agent spawns through it once.
    pub(crate) launcher: Arc<dyn Launcher>,
    /// The policy the launcher was made with, for the session's `after_session`.
    policy: Policy,
    /// The names set aside before the launch, relative to the matter.
    moved: Vec<String>,
    /// The variables the jailed agent gets over its own (the session proxy's, the CA bundle's):
    /// `Some` to set, `None` to remove. The proxy's carry its token: never logged.
    pub(crate) env: Vec<(OsString, Option<OsString>)>,
}

impl Prepared {
    /// A jailed Codex launch's mode, as its policy is ([`Sandboxed::mode`]); `None` unjailed.
    pub(crate) fn jail_mode(&self) -> Option<&'static str> {
        self.sandbox.as_ref().map(Sandboxed::mode)
    }
}

impl Sandboxed {
    /// A jailed Codex's mode, as its policy is: read-only exactly when the matter is.
    pub(crate) fn mode(&self) -> &'static str {
        if self.policy.matter_read_only() {
            crate::agent::codex::jailed::READ_ONLY
        } else {
            crate::agent::codex::JAILED_MODE
        }
    }
}

impl fmt::Debug for Sandboxed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sandboxed")
            .field("moved", &self.moved.len())
            .finish_non_exhaustive()
    }
}

/// A spawned agent's jail, on its thread's run folder until the agent has exited: the seam and
/// the launch's policy, for `after_session`, and the launcher, kept alive for exactly that
/// launch (a Seatbelt launcher owns its denial watcher registration).
#[derive(Clone)]
pub(crate) struct SandboxSession {
    launchers: Arc<dyn SandboxLaunchers>,
    launcher: Arc<dyn Launcher>,
    policy: Policy,
}

impl fmt::Debug for SandboxSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxSession").finish_non_exhaustive()
    }
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
    let grants = match policy::grants(actor, thread, &waiting) {
        Ok(grants) => grants,
        Err(error) => {
            discard(actor, thread, prepared.run, None);
            agents::fail_all(actor, thread, waiting, &error);
            return;
        }
    };
    tracing::debug!(
        tier = ?actor.protection.launch_tier(),
        native_code_deny = policy::NATIVE_CODE_DENY,
        "the agent's launch goes through the sandbox"
    );
    let Some(state) = state(actor, thread) else {
        discard(actor, thread, prepared.run, None);
        return;
    };
    let key = prepared.run.key;
    state.preparing = Some(Box::new(Preparing { id, key, waiting }));
    let running = running(actor, key);
    let clock = Arc::clone(&actor.clock);
    let denials: Arc<dyn DenialSink> = Arc::new(LoggedDenials { matter: key.matter });
    let ask = Ask {
        grants,
        running,
        opts,
        denials,
    };
    // Taken back if the seam's thread never runs (it failed to start) or unwinds (a panic).
    let held = UndoOnDrop::new(prepared, undo_prepared as fn(Box<Prepared>));
    let post = actor.post.clone();
    actor.spawn_post(async move {
        let answered = off_actor_or_undo(
            clock,
            job(key.matter),
            move || Ok(make(&*launchers, held, ask)),
            move |late: Result<Box<Prepared>, Error>| {
                if let Ok(late) = late {
                    // The names it moved still get their notice; the launch is taken back.
                    let moved = moved_names(&late);
                    undo_late(*late);
                    if !moved.is_empty() {
                        let late = SandboxInput::SetAsideLate { thread, moved };
                        let late = Input::Turn(TurnInput::Sandbox(late));
                        // The actor may be gone by then; nobody is left to tell.
                        let _gone = post.try_send(late);
                    }
                }
            },
        )
        .await;
        // The thread did not answer in time, did not start or panicked (logged there).
        let result = answered.unwrap_or_else(|_unanswered| Err(unanswered()));
        Input::Turn(TurnInput::Sandbox(SandboxInput::Sandboxed {
            thread,
            id,
            result,
        }))
    });
}

/// `value`, given to `undo` when dropped unless taken first: the seam's thread holds a launch's
/// prep in one, so a panic there or a thread that never ran takes its run folder back.
pub(super) struct UndoOnDrop<T, F: FnOnce(T)> {
    value: Option<T>,
    undo: Option<F>,
}

impl<T, F: FnOnce(T)> UndoOnDrop<T, F> {
    pub(super) const fn new(value: T, undo: F) -> Self {
        Self {
            value: Some(value),
            undo: Some(undo),
        }
    }

    /// The value, still held.
    pub(super) fn get_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }

    /// The value, handed on: no undo any more.
    pub(super) fn take(mut self) -> Option<T> {
        self.value.take()
    }
}

impl<T, F: FnOnce(T)> Drop for UndoOnDrop<T, F> {
    fn drop(&mut self) {
        if let (Some(value), Some(undo)) = (self.value.take(), self.undo.take()) {
            undo(value);
        }
    }
}

/// [`undo_late`] for a boxed prep.
fn undo_prepared(prepared: Box<Prepared>) {
    undo_late(*prepared);
}

/// The names a launch's seam moved, taken off its prep.
fn moved_names(prepared: &Prepared) -> Vec<String> {
    prepared
        .sandbox
        .as_ref()
        .map(|sandboxed| sandboxed.moved.clone())
        .unwrap_or_default()
}

/// The launcher a launch spawns through: the one the sandbox's launchers gave it (`jail`),
/// else the host's (`host`).
pub(crate) fn launcher<'a>(
    jail: Option<&'a Sandboxed>,
    host: &'a dyn Launcher,
) -> &'a dyn Launcher {
    jail.map_or(host, |jail| &*jail.launcher)
}

/// What the sandbox's launchers' work off the actor posts back ([`on_sandbox_input`]).
#[derive(Debug)]
pub(crate) enum SandboxInput {
    /// A launch's sandbox launcher `id` answered.
    Sandboxed {
        thread: ThreadId,
        id: u64,
        result: Result<Box<Prepared>, Error>,
    },
    /// Names a launch's seam moved (matter-relative) whose answer came after the wait.
    SetAsideLate {
        thread: ThreadId,
        moved: Vec<String>,
    },
    /// A sandboxed session's `after_session` ended, having moved `moved` (matter-relative
    /// names), and then its run folder's removal, as `TurnInput::RunRemoved` says.
    SessionEnded {
        thread: ThreadId,
        key: SessionKey,
        freed: bool,
        moved: Vec<String>,
    },
}

/// Takes what the sandbox's launchers' work posted back.
pub(crate) fn on_sandbox_input(actor: &mut Actor, input: SandboxInput) {
    match input {
        SandboxInput::Sandboxed { thread, id, result } => on_sandboxed(actor, thread, id, result),
        SandboxInput::SetAsideLate { thread, moved } => on_set_aside_late(actor, thread, &moved),
        SandboxInput::SessionEnded {
            thread,
            key,
            freed,
            moved,
        } => on_session_ended(actor, thread, key, freed, &moved),
    }
}

/// The keys of `key`'s matter's other sessions that may run an agent: every other thread of
/// the matter with a launch in its checks or at the seam (`preparing`), or holding a run folder
/// (from its agent's spawn until the session is discarded), sorted. A launch still being
/// prepared counts, so a concurrent launch never takes it for a crashed one.
pub(super) fn running(actor: &Actor, key: SessionKey) -> Vec<SessionKey> {
    let mut running: Vec<SessionKey> = actor
        .threads
        .values()
        .flat_map(|entry| {
            let state = &entry.turn;
            let preparing = state.preparing.as_ref().map(|preparing| preparing.key);
            preparing
                .into_iter()
                .chain(state.run.as_ref().map(|run| run.key))
        })
        .filter(|other| other.matter == key.matter && other.thread != key.thread)
        .collect();
    running.sort_unstable();
    running.dedup();
    running
}

/// What the seam is asked with besides the launch: what the policy takes (`policy::Grants`), the
/// matter's other running sessions, the spawn options and the denial sink.
struct Ask {
    grants: Grants,
    running: Vec<SessionKey>,
    opts: SpawnOptions,
    denials: Arc<dyn DenialSink>,
}

/// On the seam's thread: the reservation, the jail (its runtime folders, CA bundle, policy and
/// variables, `policy::jail`), then the seam's launcher with the reservation. On a failure the run
/// folder is taken back here, before the answer goes out, and a reservation made is dropped
/// (released) here too.
fn make(
    launchers: &dyn SandboxLaunchers,
    mut held: UndoOnDrop<Box<Prepared>, fn(Box<Prepared>)>,
    ask: Ask,
) -> Result<Box<Prepared>, Error> {
    let Some(prepared) = held.get_mut() else {
        return Err(unanswered());
    };
    let key = prepared.run.key;
    let matter = key.matter;
    let made = launchers
        .reserve(&matter, &key, &ask.running)
        .map_err(|error| fail(matter, &error))
        .and_then(|reservation| {
            let jail = policy::jail(prepared, &ask.grants, reservation.ports())?;
            let launch = launchers
                .launcher(
                    jail.policy.clone(),
                    &matter,
                    &key,
                    reservation,
                    ask.opts,
                    ask.denials,
                )
                .map_err(|error| fail(matter, &error))?;
            Ok((jail, launch))
        });
    match made {
        Ok((jail, launch)) => {
            let moved = notice::relative_names(&launch.set_aside, jail.policy.matter());
            prepared.sandbox = Some(Sandboxed {
                launcher: launch.launcher,
                policy: jail.policy,
                moved,
                env: jail.env,
            });
            held.take().ok_or_else(unanswered)
        }
        Err(error) => {
            tracing::warn!(%matter, "the sandbox did not prepare the agent's launch");
            // The run folder goes here, on the seam's thread.
            drop(held);
            Err(error)
        }
    }
}

/// A seam's `error`, logged by its kind only, as the launch's failure.
fn fail(matter: MatterId, error: &clauseo_sandbox::Error) -> Error {
    let kind = kind(error);
    tracing::warn!(%matter, kind, "the sandbox refused the agent's launch");
    refused(error)
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

/// The launch's failure for a seam's error: the error's own `code()` (`agent.launch_failed`,
/// `matter.protected_link` or `internal`); the message names the kind only.
fn refused(error: &clauseo_sandbox::Error) -> Error {
    Error::refused(
        error.code(),
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
        // An answer nobody waits for (a stop, a sign-in change): its names still get their notice.
        if let Ok(stale) = result {
            let moved = moved_names(&stale);
            discard(actor, thread, stale.run, None);
            on_set_aside_late(actor, thread, &moved);
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
        on_set_aside_late(actor, thread, &moved);
        discard(actor, thread, prepared.run, None);
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    }
    on_set_aside_late(actor, thread, &moved);
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
            policy: sandboxed.policy.clone(),
        });
    super::launch_prepared(actor, thread, waiting, prepared, session);
}

/// The session `key`'s agent has exited: the seam's `after_session` with the launch's policy,
/// once, off the actor, bounded, and the launch's launcher dropped there after it; then
/// `removal`, the run folder's, so the seam sees the session's folders as the agent left them
/// (the removal is counted for shutdown by the caller, and this ends it). What it moved comes
/// back matter-relative for [`on_session_ended`].
pub(super) fn after_session(
    actor: &mut Actor,
    thread: ThreadId,
    key: SessionKey,
    session: SandboxSession,
    removal: impl Future<Output = bool> + 'static,
) {
    let clock = Arc::clone(&actor.clock);
    let job = Job::short("the sandbox's after-session", Some(key.matter));
    actor.spawn_post(async move {
        let done = off_actor(clock, job, move || {
            let SandboxSession {
                launchers,
                launcher,
                policy,
            } = session;
            let root = policy.matter().to_owned();
            let moved = launchers.after_session(SessionEnd::new(key.matter, key, policy));
            drop(launcher);
            Ok(notice::relative_names(&moved, &root))
        })
        .await;
        let moved = done.unwrap_or_else(|error| {
            let error = Cause(&error);
            tracing::warn!(%thread, %error, "the sandbox's after-session call did not answer");
            Vec::new()
        });
        let freed = removal.await;
        Input::Turn(TurnInput::Sandbox(SandboxInput::SessionEnded {
            thread,
            key,
            freed,
            moved,
        }))
    });
}

/// Names a launch moved (matter-relative): one notice while the host serves, a count in the log
/// during shutdown.
pub(crate) fn on_set_aside_late(actor: &mut Actor, thread: ThreadId, moved: &[String]) {
    if moved.is_empty() {
        return;
    }
    if host_stopping(actor) {
        let moved = moved.len();
        tracing::info!(%thread, moved, "the sandbox set names aside; the host stops");
        return;
    }
    notice::log(actor, thread, moved, notice::When::BeforeLaunch);
}

/// A session's `after_session` and then its run folder's removal ended: the removal ends as
/// [`super::on_run_removed`] says (`freed`: whether the run was freed), and the names it moved
/// (matter-relative) give one notice on the thread while the host serves.
pub(crate) fn on_session_ended(
    actor: &mut Actor,
    thread: ThreadId,
    key: SessionKey,
    freed: bool,
    moved: &[String],
) {
    super::on_run_removed(actor, key, freed);
    if moved.is_empty() {
        return;
    }
    if host_stopping(actor) {
        let moved = moved.len();
        tracing::info!(%thread, moved, "the sandbox moved names after a session; host stops");
        return;
    }
    notice::log(actor, thread, moved, notice::When::AfterSession);
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

mod ca;
mod notice;
mod over_cap;
mod policy;
use policy::Grants;
#[cfg(test)]
mod tests;
