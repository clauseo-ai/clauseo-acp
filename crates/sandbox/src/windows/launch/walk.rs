//! The preparation's walk, on its own thread. The order, after `reserve`'s bookkeeping:
//!
//! 1. the gate and the plan (reads only);
//! 2. the launch check (the admin copy, the clean-close record, the journal);
//! 3. the network check;
//! 4. leftovers: a process running as the pool user (an earlier run's) is stopped by the stop
//!    task and the pool user's sweep, then looked for again, until none is left or the waits'
//!    budget runs out ([`Error::LeftoverProcess`]); a leftover isn't the lawyer's to deal with;
//! 5. the set-aside's launch pass;
//! 6. `PrepareRun{slot}` on the SYSTEM side, retried while a set-up holds its lock;
//! 7. `apply`: the walk, cancellable at each folder.
//!
//! Nothing is granted to the pool user or run as it before the launch and network checks
//! passed: the stop, the sweep (which runs as the pool user), the set-aside's pass, `PrepareRun`
//! (which adds it to the run's group) and `apply` come after them.
//!
//! **Two limits.** The waits (step 4's stops, sweeps and re-checks, step 6's retries) share one
//! wall-clock budget, [`PREPARE_BOUND`]; running out refuses the launch. The work steps (1, 2, 5
//! and 7) take as long as the matter's size needs, with no wall-clock cap, but cancel once their
//! progress hasn't moved for [`PREPARE_STALL`] ([`Error::PrepareStalled`], undone as any
//! failure). The walk looks at its cancel and its progress before and after every step, and
//! `apply` sees both through [`Walk::cancelled`] at each folder. Once `apply` returns, the
//! cancel is looked at again (and once more under the registry's lock): a cancel that came
//! during or after `apply` undoes and is never prepared. A step blocked in one OS call can't be
//! interrupted from here, so **the real steps (C5) bound their own waits** (every wait a timed
//! one) and tick their progress every few seconds; a step that doesn't return makes its matter
//! [`Error::MatterStuck`] after [`STUCK_GRACE`] (`registry.rs`).
//!
//! [`PREPARE_STALL`]: super::PREPARE_STALL
//! [`STUCK_GRACE`]: super::STUCK_GRACE

use std::cell::Cell;
use std::time::Duration;

use super::RETRY_PAUSE;
use super::prepare::{PREPARE_BOUND, PREPARE_STALL, Reserved, Shared, Stage, Walk};
use super::registry::Registry;
use super::steps::{LaunchSteps, RunRefusal, Slot, Stopped};
use crate::windows::pool::PoolUser;
use crate::{Blocked, Error, Policy};

/// The walk's look at its cancel and its progress, before and after each step, and the waits'
/// budget spent so far.
struct Gate<'a, S: LaunchSteps> {
    steps: &'a S,
    shared: &'a Shared,
    /// What the waits have taken of [`PREPARE_BOUND`].
    waited: Cell<Duration>,
}

impl<S: LaunchSteps> Gate<'_, S> {
    /// [`Error::Cancelled`] once cancelled; [`Error::PrepareStalled`] once its progress hasn't
    /// moved for [`PREPARE_STALL`]. Otherwise a step's end counts as progress.
    fn go_on(&self) -> Result<(), Error> {
        self.shared.go_on()?;
        let now = self.steps.now();
        if self.shared.stalled(now) {
            tracing::warn!(stall = ?PREPARE_STALL, "tier B's preparation stopped making progress");
            return Err(Error::PrepareStalled);
        }
        self.shared.tick(now);
        Ok(())
    }

    /// Moves to `stage`, then [`Gate::go_on`].
    fn at(&self, stage: Stage) -> Result<(), Error> {
        self.shared.set_stage(stage);
        self.go_on()
    }
}

/// One wait's share of the budget: what it takes is added to the gate's when it ends.
struct Wait<'g, 'a, S: LaunchSteps> {
    gate: &'g Gate<'a, S>,
    began: Duration,
}

impl<'g, 'a, S: LaunchSteps> Wait<'g, 'a, S> {
    fn start(gate: &'g Gate<'a, S>) -> Wait<'g, 'a, S> {
        Wait {
            gate,
            began: gate.steps.now(),
        }
    }

    /// Whether the waits, this one included, have spent [`PREPARE_BOUND`].
    fn over(&self) -> bool {
        let spent = self.gate.steps.now().saturating_sub(self.began);
        self.gate.waited.get().saturating_add(spent) >= PREPARE_BOUND
    }

    /// [`Error::Cancelled`] once cancelled. A wait is bounded by the budget, not the stall
    /// timer, so each of its tries counts as progress.
    fn alive(&self) -> Result<(), Error> {
        self.gate.shared.go_on()?;
        self.gate.shared.tick(self.gate.steps.now());
        Ok(())
    }
}

impl<S: LaunchSteps> Drop for Wait<'_, '_, S> {
    fn drop(&mut self) {
        let spent = self.gate.steps.now().saturating_sub(self.began);
        let waited = self.gate.waited.get().saturating_add(spent);
        self.gate.waited.set(waited);
    }
}

/// The walk's steps, in order; see the module's docs.
pub(super) fn walk<S: LaunchSteps>(
    registry: &Registry<S>,
    reserved: &Reserved,
    policy: &Policy,
    shared: &Shared,
) -> Result<(), Error> {
    let steps = &*registry.steps;
    let gate = Gate {
        steps,
        shared,
        waited: Cell::new(Duration::ZERO),
    };
    let now = || steps.now();
    let walk = Walk { shared, now: &now };
    let Reserved {
        id,
        matter,
        session,
        pool,
        slot,
    } = reserved;
    let (id, matter) = (*id, matter.as_str());
    let running = registry
        .update(matter, id, |entry| entry.running.clone())
        .ok_or(Error::WrongReservation)?;
    let running: Vec<&str> = running.iter().map(String::as_str).collect();
    gate.at(Stage::Checking)?;
    let plan = steps.plan(policy, pool, &walk)?;
    gate.go_on()?;
    let check = steps.check_launch(&plan, &walk)?;
    gate.go_on()?;
    steps.check_network(policy, pool)?;
    gate.go_on()?;
    stop_leftovers(&gate, pool)?;
    gate.at(Stage::SettingAside)?;
    let moved = steps.before_launch(&plan, &check, session, &running, &walk)?;
    registry.update(matter, id, |entry| entry.moved = moved);
    gate.at(Stage::PreparingRun)?;
    let group = prepare_run(&gate, slot, pool)?;
    // The entry holds the group, so the undo ends it; with no entry, it is ended here.
    let mut group = Some(group);
    registry.update(matter, id, |entry| entry.run = group.take());
    if group.is_some() {
        registry.end_run_and_free_slot(Some(pool), group, None);
        return Err(Error::WrongReservation);
    }
    gate.at(Stage::Walking)?;
    let applied = steps.apply(&plan, &check, &walk);
    // A cancel or a stall during `apply` is the walk's error, not `apply`'s own stop; the
    // registry looks at the cancel once more before it marks the claim prepared.
    gate.go_on()?;
    applied
}

/// Stops the pool user's leftover processes: the stop task, then its sweep, then a fresh look;
/// again every [`RETRY_PAUSE`] while any survives, until the waits' budget runs out, when the
/// launch is refused with [`Error::LeftoverProcess`].
fn stop_leftovers<S: LaunchSteps>(gate: &Gate<'_, S>, pool: &PoolUser) -> Result<(), Error> {
    let (steps, shared) = (gate.steps, gate.shared);
    let wait = Wait::start(gate);
    let mut found = steps.pool_user_processes(pool)?;
    while !found.is_empty() {
        wait.alive()?;
        if wait.over() {
            tracing::warn!(
                count = found.len(),
                "tier B's pool user kept a leftover process"
            );
            return Err(Error::LeftoverProcess { count: found.len() });
        }
        shared.set_stage(Stage::Stopping);
        let Stopped { ended } = steps.stop_pool_user(pool)?;
        shared.stopped(ended);
        tracing::info!(
            found = found.len(),
            ended,
            "tier B stopped a pool user's leftovers"
        );
        wait.alive()?;
        steps.sweep_pool_user(pool)?;
        wait.alive()?;
        found = steps.pool_user_processes(pool)?;
        if !found.is_empty() {
            steps.pause(RETRY_PAUSE);
        }
    }
    Ok(())
}

/// `PrepareRun`, tried again every [`RETRY_PAUSE`] while a set-up runs, until the waits'
/// budget runs out; then a plain failure. A group it made is the caller's to record before it
/// looks at the cancel again.
fn prepare_run<S: LaunchSteps>(
    gate: &Gate<'_, S>,
    slot: &Slot,
    pool: &PoolUser,
) -> Result<S::RunGroup, Error> {
    let steps = gate.steps;
    let wait = Wait::start(gate);
    loop {
        match steps.prepare_run(slot, pool) {
            Ok(group) => return Ok(group),
            Err(RunRefusal::SetUpRunning) => {
                wait.alive()?;
                if wait.over() {
                    tracing::warn!("tier B's preparation gave up waiting for a running set-up");
                    return Err(Error::Launch(clauseo_process::Error::Os {
                        what: "the sandbox's set-up is still running",
                        source: std::io::ErrorKind::TimedOut.into(),
                    }));
                }
                steps.pause(RETRY_PAUSE);
                wait.alive()?;
            }
            Err(RunRefusal::LooseTask) => return Err(Error::Blocked(Blocked::LooseTask)),
            Err(RunRefusal::Failed(why)) => {
                tracing::warn!(%why, "tier B's PrepareRun failed");
                return Err(Error::Launch(clauseo_process::Error::Os {
                    what: "the sandbox couldn't prepare the run",
                    source: std::io::Error::other(why),
                }));
            }
        }
    }
}
