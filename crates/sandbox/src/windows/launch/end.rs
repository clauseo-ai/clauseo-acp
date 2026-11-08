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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::prepare::Walk;
use super::registry::{FinishEnd, Registry};
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
    let steps = &*registry.steps;
    let ended = registry.end(matter, session, id)?;
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
    Some(moved)
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
    /// The outer has started: the session's end is the pass from now on, and it waits for the
    /// pool user's processes.
    pub(super) fn started(&self) {
        self.registry
            .update(&self.matter, self.id, |entry| entry.spawned = true);
        self.armed.store(true, Ordering::Release);
    }
}

impl<S: LaunchSteps> Drop for SessionGuard<S> {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Acquire) || !self.registry.launched(&self.matter, self.id) {
            return;
        }
        let registry = Arc::clone(&self.registry);
        let (matter, session) = (self.matter.clone(), self.session.clone());
        let (id, policy) = (self.id, self.policy.clone());
        registry.end_started();
        let run = move || {
            end_session(&registry, (&matter, &session, Some(id)), &policy);
            registry.end_done();
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
