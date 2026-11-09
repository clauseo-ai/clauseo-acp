//! A tier B session's end: what the outer's exit means to the host ([`RunEnd`]), and the
//! after-session pass, which runs once per session whose agent started, whoever asks first:
//! the host's [`after_session`](super::Launchers::after_session) once the agent's tree is
//! gone, or the launcher's guard when the host drops the launcher without one (a quit).
//!
//! **One end per session.** Until its outer starts, the launcher holds the reservation's claim
//! (`tier_b.rs`): a failed start, or a launcher dropped unused, undoes the session (the
//! registry's release) and no pass runs. Once the outer started the claim is let go of and
//! the guard armed: the pass is the session's one end. Both take the registry's entry out of
//! launched first (`Registry::end`, `Registry::release_now`), so whichever comes first wins
//! and the other finds nothing. The pass's `EndRun` and [`Finish`] are drop guards: a
//! panicking step still ends the run's group, gives the slot back and frees the matter.
//!
//! The pass waits first for every process of the pool user's to have truly exited
//! ([`LaunchSteps::wait_exited`]), not for the job to read empty (`TreeWatch::gone`), which on
//! Windows it does while processes still tear down (stage D, #697). One that outlives
//! [`EXIT_WAIT`] gets the stop task and a second wait; then the pass goes on, and the next
//! preparation's leftover check meets what is left.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::prepare::Walk;
use super::registry::{Ended, FinishEnd, OuterState, Registry, State, raced};
use super::steps::{Ending, LaunchSteps, Slot};
use crate::windows::pool::PoolUser;
use crate::windows::runner::outcome::Meaning;
use crate::windows::runner::{RunExit, RunOutcome};
use crate::{Error, NoSandbox, Policy, SetAside};

/// How long the end waits for the pool user's processes to exit, once, and again after the
/// stop task for one that didn't.
pub const EXIT_WAIT: Duration = Duration::from_secs(10);

/// What the host does once the outer runner has exited: [`RunEnd::from_exit`] on its
/// `ExitReport::code()`. The host's adapter reads it before it hands the session's end to
/// [`after_session`](super::Launchers::after_session), which runs whatever it says.
#[derive(Debug)]
pub enum RunEnd {
    /// The agent ran and exited with this code: the host's usual handling of an agent's exit.
    Exited(u32),
    /// The host's own shutdown ladder ended the run (a cancel): nothing more to report.
    HostEnded,
    /// A sweep found a process of the pool user's that no sweep could end: the host's "left
    /// the Safe workspace" stop (R17 PR-E), whatever the agent's code. The outer has asked the
    /// stop task itself.
    LeftSafeWorkspace,
    /// A set-up failure: the host detects the tier again with this cause.
    Redetect(NoSandbox),
    /// This launch failed, for this reason: the error's `code()` is `agent.launch_failed`, and
    /// its `kind()` the words with no path.
    Failed(Error),
}

impl RunEnd {
    /// What the outer's exit status means: `code` is `ExitStatus::code()`.
    pub fn from_exit(code: Option<i32>) -> RunEnd {
        RunEnd::from_outcome(RunOutcome::from_exit(code))
    }

    /// What a decoded exit means (0049's PR6 end).
    ///
    /// TODO(PR6 rebase over #717 and #757): `RunExit::AwaitingStop` and `AwaitingSweep` (a
    /// stop or a sweep of the pool user's still running) become `Failed(LeftoverProcess)`, and
    /// `RunExit::LeftoverTask` becomes `Failed(Blocked(LeftoverTask))`, through their own
    /// `Meaning`s. Until then they decode as `Unknown`, which fails the launch plainly.
    pub fn from_outcome(outcome: RunOutcome) -> RunEnd {
        match outcome.meaning() {
            Meaning::AgentExited(code) => RunEnd::Exited(code),
            Meaning::HostEnded => RunEnd::HostEnded,
            Meaning::LeftSafeWorkspace => RunEnd::LeftSafeWorkspace,
            Meaning::Redetect(cause) => RunEnd::Redetect(cause),
            Meaning::LaunchFailed(exit) => RunEnd::Failed(runner_failed(exit)),
        }
    }
}

/// The launch's failure for the runner's `exit` (`None`: a code this build doesn't know).
fn runner_failed(exit: Option<RunExit>) -> Error {
    let said = exit.map_or_else(
        || "an exit code this version doesn't know".to_owned(),
        |exit| format!("{exit:?} ({:#x})", exit.code()),
    );
    Error::Launch(clauseo_process::Error::Os {
        what: "the sandbox's runner failed",
        source: std::io::Error::other(said),
    })
}

/// The after-session pass for `session` of `matter` (of claim `id`, when given), if it is
/// launched and its end hasn't started: what the set-aside's pass moved. `None` otherwise, so
/// a second ask does nothing. In order: the wait for the pool user's processes (when the outer
/// started), the journal's cursor, the repair, the end of R18's reads, the set-aside's pass and
/// the clean-close record; then, from drop guards, `EndRun`, the slot and the matter's release.
pub(super) fn end_session<S: LaunchSteps>(
    registry: &Registry<S>,
    (matter, session, id): (&str, &str, Option<u64>),
    policy: &Policy,
) -> Option<Vec<SetAside>> {
    let ended = registry.end(matter, session, id)?;
    if ended.handoff {
        // The end raced the outer's start: nothing moves (no repair, no set-aside) until the
        // launcher has ended the outer and seen its job truly empty; it runs the pass then
        // (`SessionGuard::finish_race`), and what that pass moves is only logged.
        tracing::info!("tier B's session end raced its outer's start; the launcher ends it");
        return Some(Vec::new());
    }
    Some(pass(registry, (matter, session), ended, policy))
}

/// The after-session pass over `ended`: see [`end_session`].
fn pass<S: LaunchSteps>(
    registry: &Registry<S>,
    (matter, session): (&str, &str),
    ended: Ended<S>,
    policy: &Policy,
) -> Vec<SetAside> {
    let steps = &*registry.steps;
    // Dropped in reverse: the run's group and the slot first, then the matter freed.
    let _finish = FinishEnd {
        registry,
        matter,
        id: ended.id,
    };
    let _end_run = EndRun {
        registry,
        pool: ended.pool.clone(),
        run: ended.run,
        slot: ended.slot,
    };
    registry.files.retry(steps);
    if let Some(file) = &ended.launch_file {
        registry.files.remove(steps, file);
    }
    let mut moved = Vec::new();
    if let Some(pool) = &ended.pool {
        if ended.spawned {
            wait_exited(steps, pool);
        }
        let now = || steps.now();
        let walk = Walk {
            shared: &ended.walk,
            now: &now,
        };
        // Each step's end is progress, from this (the walking) thread: a stalled step stops,
        // and the next gets its own time. The work steps count their own items through
        // `end.walk` (`Walk::advance`).
        let stepped = || ended.walk.tick(steps.now());
        stepped();
        let end = Ending {
            walk: &walk,
            matter,
            session,
            policy,
            pool,
        };
        let cursor = steps.session_cursor(&end);
        stepped();
        let repaired = steps.repair(&end);
        stepped();
        if let Err(error) = &repaired {
            tracing::warn!(
                kind = error.kind(),
                "tier B's repair after the session failed"
            );
        }
        steps.end_reads(&end);
        stepped();
        let passed = steps.set_aside_after(&end);
        match &passed {
            Ok(passed) => moved = steps.moved(passed),
            Err(error) => {
                tracing::warn!(
                    kind = error.kind(),
                    "tier B's set-aside after the session failed"
                );
            }
        }
        steps.write_clean_close(&end, cursor.as_ref(), &repaired, &passed);
    }
    moved
}

/// Ends the run's group, then gives the slot back, when dropped: at the end of the pass, or on
/// a panic in one of its steps. A failed `EndRun` keeps both held, and the pool user held back,
/// until a later one succeeds (`Registry::end_run_and_free_slot`).
struct EndRun<'a, S: LaunchSteps> {
    registry: &'a Registry<S>,
    pool: Option<PoolUser>,
    run: Option<S::RunGroup>,
    slot: Option<Slot>,
}

impl<S: LaunchSteps> Drop for EndRun<'_, S> {
    fn drop(&mut self) {
        let (run, slot) = (self.run.take(), self.slot.take());
        self.registry
            .end_run_and_free_slot(self.pool.as_ref(), run, slot);
    }
}

/// Waits for the pool user's processes to have truly exited; the stop task for any left after
/// [`EXIT_WAIT`], and a second wait. What still runs then is logged, not waited for.
fn wait_exited<S: LaunchSteps>(steps: &S, pool: &PoolUser) {
    let left = match steps.wait_exited(pool, EXIT_WAIT) {
        Ok(left) if left.is_empty() => return,
        Ok(left) => left.len(),
        Err(error) => {
            tracing::warn!(
                kind = error.kind(),
                "tier B couldn't wait for the agent's end"
            );
            return;
        }
    };
    tracing::info!(
        left,
        "tier B stops the pool user's processes left at the end"
    );
    if let Err(error) = steps.stop_pool_user(pool) {
        tracing::warn!(kind = error.kind(), "tier B's stop at the end failed");
    }
    match steps.wait_exited(pool, EXIT_WAIT) {
        Ok(left) if left.is_empty() => {}
        Ok(left) => {
            // TODO(C5, launch-blocking; 0049's C5 list): don't free the slot or the matter
            // while the pool user still runs processes. Quarantine the slot (a registry state
            // between ending and free, `reserve` still MatterBusy), end the processes by stop
            // and sweep again, and free it only once it is truly empty by #697's semantics
            // (`wait_exited` empty). Today the pass goes on and frees both.
            tracing::warn!(
                left = left.len(),
                "the pool user still runs processes after the session"
            );
        }
        Err(error) => {
            tracing::warn!(
                kind = error.kind(),
                "tier B couldn't wait for the agent's end"
            );
        }
    }
}

/// The launcher's hold on its session's end, armed once the outer has started
/// ([`SessionGuard::started`]). Dropped armed while the session is launched and its end not
/// started, it runs the after-session pass on a thread of its own (the drop doesn't block),
/// counted until done ([`Launchers::wait_ends`](super::Launchers::wait_ends)). Unarmed, it
/// does nothing: the launcher's claim undoes a session whose agent never started. A quit that
/// ends the process before the pass finishes leaves what a crash leaves: the next start's
/// `start_up` and launch pass deal with it.
pub(super) struct SessionGuard<S: LaunchSteps> {
    pub(super) registry: Arc<Registry<S>>,
    pub(super) matter: String,
    pub(super) session: String,
    pub(super) id: u64,
    pub(super) policy: Policy,
    pub(super) armed: AtomicBool,
}

impl<S: LaunchSteps> SessionGuard<S> {
    /// The outer is about to be started: `false` when the session's end has begun (nothing is
    /// started then). From here an end hands the slot and the matter over to the launcher.
    pub(super) fn spawning(&self) -> bool {
        self.registry
            .update(&self.matter, self.id, |entry| {
                let launched = entry.state == State::Launched;
                if launched {
                    entry.outer = OuterState::Spawning;
                }
                launched
            })
            .unwrap_or(false)
    }

    /// The spawn has returned (`started`: an outer runs): what the launcher does next.
    pub(super) fn spawned(&self, started: bool) -> AfterSpawn {
        let next = self
            .registry
            .update(&self.matter, self.id, |entry| {
                if entry.outer == OuterState::Raced {
                    AfterSpawn::Raced
                } else if started && entry.state == State::Launched {
                    entry.outer = OuterState::Spawned;
                    AfterSpawn::Run
                } else {
                    entry.outer = OuterState::Unstarted;
                    AfterSpawn::Failed
                }
            })
            .unwrap_or(AfterSpawn::Failed);
        // From here the session's end is the pass, and it waits for the pool user's processes.
        self.armed.store(next == AfterSpawn::Run, Ordering::Release);
        next
    }

    /// The end raced the start: once the outer is `gone` (ended and its job seen truly empty),
    /// runs the end's pass here, which frees the slot and the matter; else leaves the matter
    /// stuck, with no repair and no set-aside.
    pub(super) fn finish_race(&self, gone: bool) {
        let Some(ended) = raced(&self.registry, &self.matter, self.id, gone) else {
            return;
        };
        let moved = pass(
            &self.registry,
            (&self.matter, &self.session),
            ended,
            &self.policy,
        );
        tracing::info!(
            moved = moved.len(),
            "tier B ended a session whose end raced its start"
        );
    }

    /// Notes the session's launch file, which its end or undo deletes if nothing has.
    pub(super) fn launch_file(&self, file: &Path) {
        self.registry.update(&self.matter, self.id, |entry| {
            entry.launch_file = Some(file.to_path_buf());
        });
    }

    /// The session's note of its launch file, taken.
    pub(super) fn take_launch_file(&self) -> Option<PathBuf> {
        self.registry
            .update(&self.matter, self.id, |entry| entry.launch_file.take())
            .flatten()
    }

    /// [`take_launch_file`](Self::take_launch_file), for another thread.
    pub(super) fn file_taker(&self) -> impl Fn() -> Option<PathBuf> + Send + 'static {
        let registry = Arc::clone(&self.registry);
        let (matter, id) = (self.matter.clone(), self.id);
        move || {
            registry
                .update(&matter, id, |entry| entry.launch_file.take())
                .flatten()
        }
    }
}

/// What follows a spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterSpawn {
    /// The outer runs as the session's agent.
    Run,
    /// The spawn failed: the launcher's claim undoes the session.
    Failed,
    /// The session's end began during the spawn: the outer (if any) is ended, then
    /// [`SessionGuard::finish_race`].
    Raced,
}

/// Counts an end on a thread of its own done when dropped, even if the pass panicked.
struct EndDone<'a, S: LaunchSteps>(&'a Registry<S>);

impl<S: LaunchSteps> Drop for EndDone<'_, S> {
    fn drop(&mut self) {
        self.0.ends.done();
    }
}

impl<S: LaunchSteps> Drop for SessionGuard<S> {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Acquire) || !self.registry.is_launched(self.id) {
            return;
        }
        let registry = Arc::clone(&self.registry);
        let (matter, session) = (self.matter.clone(), self.session.clone());
        let (id, policy) = (self.id, self.policy.clone());
        registry.ends.started();
        let run = move || {
            let _done = EndDone(&registry);
            end_session(&registry, (&matter, &session, Some(id)), &policy);
        };
        // A thread that won't start can't take the closure back: the end runs here instead.
        let once = Arc::new(std::sync::Mutex::new(Some(run)));
        let theirs = Arc::clone(&once);
        let started = std::thread::Builder::new()
            .name("clauseo-tierb-end".to_owned())
            .spawn(move || take(&theirs).map(|run| run()));
        if started.is_err() {
            tracing::warn!("tier B's end thread didn't start; the end runs here");
            if let Some(run) = take(&once) {
                run();
            }
        }
    }
}

/// What `slot` holds, taken.
fn take<T>(slot: &std::sync::Mutex<Option<T>>) -> Option<T> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// How many ends dropped launchers started on threads of their own are still running.
#[derive(Default)]
pub(super) struct Ends {
    running: std::sync::Mutex<usize>,
    done: std::sync::Condvar,
}

impl Ends {
    /// Counts one started.
    fn started(&self) {
        *self.count() += 1;
    }

    /// Counts one done.
    fn done(&self) {
        let mut running = self.count();
        *running = running.saturating_sub(1);
        drop(running);
        self.done.notify_all();
    }

    /// Waits up to `timeout` for them: whether none is left.
    pub(super) fn wait(&self, timeout: Duration) -> bool {
        let running = self.count();
        let (running, _) = self
            .done
            .wait_timeout_while(running, timeout, |running| *running > 0)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *running == 0
    }

    fn count(&self) -> std::sync::MutexGuard<'_, usize> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Launch files whose removal failed (`LaunchSteps::remove_launch`), tried again at each end and
/// undo; the next start's stale sweep (`clear_stale`) removes any left. Logged with no content.
#[derive(Default)]
pub(super) struct LaunchFiles(std::sync::Mutex<Vec<PathBuf>>);

impl LaunchFiles {
    /// Removes `file`, or notes it to try again.
    pub(super) fn remove<S: LaunchSteps>(&self, steps: &S, file: &Path) {
        if let Err(error) = steps.remove_launch(file) {
            tracing::warn!(
                kind = error.kind(),
                "tier B couldn't remove a launch file; noted"
            );
            self.noted().push(file.to_path_buf());
        }
    }

    /// Tries the noted files again.
    pub(super) fn retry<S: LaunchSteps>(&self, steps: &S) {
        let noted = std::mem::take(&mut *self.noted());
        for file in noted {
            self.remove(steps, &file);
        }
    }

    fn noted(&self) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
