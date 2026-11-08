//! The matter's preparation, in the background: the walk `prepare` starts for a reservation,
//! and the handle the host polls ([`Preparing`]). The order, after `reserve`'s bookkeeping:
//!
//! 1. the gate and the plan (reads only);
//! 2. the launch check (the admin copy, the clean-close record, the journal);
//! 3. the network check;
//! 4. leftovers: a process running as the pool user (an earlier run's) is stopped by the stop
//!    task and the pool user's sweep, then looked for again, until none is left or the bound
//!    passes ([`Error::LeftoverProcess`]); a leftover isn't the lawyer's to deal with;
//! 5. the set-aside's launch pass;
//! 6. `PrepareRun{slot}` on the SYSTEM side, retried while a set-up holds its lock;
//! 7. `apply`: the walk, cancellable at each folder.
//!
//! Nothing is granted to the pool user or run as it before the launch and network checks
//! passed: the stop, the sweep (which runs as the pool user), the set-aside's pass, `PrepareRun`
//! (which adds it to the run's group) and `apply` come after them.
//!
//! **One bound covers the whole walk:** [`PREPARE_BOUND`] from its start. The walk looks at the
//! clock and at its cancel before and after every step (each stop, sweep and `PrepareRun` try
//! included), and `apply` sees both through [`Walk::cancelled`] at each folder; past the bound
//! it fails (`LeftoverProcess` while a leftover survives, the set-up's words while one runs,
//! else a plain timeout). Once `apply` returns, the cancel is looked at again, under the
//! registry's lock: a cancel that came during or after `apply` undoes and is never prepared.
//! A step blocked in one OS call can't be interrupted from here, so **the real steps (C5) bound
//! their own waits**: every wait (a scheduled task, the set-up server's pipe, a process handle,
//! a file lock) is a timed one, no longer than the bound, and `apply` checks `Walk::cancelled`
//! at each folder.
//!
//! The walk borrows nothing from the host: it is keyed by the reservation's claim and matter in
//! the registry. A failure, and a cancel, undo what was done for the session on the walk's own
//! thread (the group, the slot, the session's mark) before the result is posted; the
//! reservation stays the host's, refused by `launch` ([`Error::NotPrepared`]) until it drops
//! it. Dropping the reservation mid-walk cancels the walk, which undoes and frees the matter
//! when it stops; so does dropping every [`Preparing`] handle of it (an abandoned walk). The
//! permission entries `apply` wrote stay in the ledger, so the next preparation continues
//! from them. TODO(C5): those entries must name the run's group, not the pool user, so a
//! grant never outlives a failed check (0049's PR6 C5 gate).

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::registry::Registry;
use super::steps::{LaunchSteps, RunRefusal, Slot, Stopped};
use crate::windows::pool::PoolUser;
use crate::{Blocked, Error, Policy};

/// How long a whole preparation may take, from the walk's start, its leftover stops and its
/// `PrepareRun` retries included: the host's launch bound on tier B (0049). Past it, it fails.
pub const PREPARE_BOUND: Duration = Duration::from_secs(180);
/// How long it waits between those tries, and before it stops a leftover again.
pub const RETRY_PAUSE: Duration = Duration::from_secs(1);

/// Which step a preparation is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The gate, the plan, the launch and network checks, the look for leftovers.
    Checking,
    /// Stopping the pool user's leftovers: the stop task and the sweep.
    Stopping,
    /// The set-aside's launch pass.
    SettingAside,
    /// `PrepareRun`, waiting on a running set-up included.
    PreparingRun,
    /// The walk.
    Walking,
    /// Done: its result waits in [`Preparing::result`], or was taken.
    Finished,
}

/// How far a preparation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The step.
    pub stage: Stage,
    /// The folders the walk has done.
    pub walked: u64,
    /// The leftover processes the stop task ended, as it reported.
    pub stopped: u64,
    /// Whether it has finished.
    pub done: bool,
}

/// A matter's preparation, running in the background. [`Preparing::cancel`] stops the walk, and
/// so do dropping the reservation and **dropping its last handle** (an abandoned walk cancels:
/// the host keeps a handle for as long as it wants the walk). Another `prepare` of the same
/// reservation while it runs joins it, with a handle of its own.
pub struct Preparing {
    shared: Arc<Shared>,
}

/// A walk's state, shared by its thread, its handles and the registry (which cancels it when
/// the reservation is dropped). Its lock is held only to read or post the stage and result.
pub(super) struct Shared {
    inner: Mutex<Inner>,
    finished: Condvar,
    cancelled: AtomicBool,
    /// The [`Preparing`] handles of a running walk: the last one's drop cancels it.
    handles: AtomicUsize,
    walked: AtomicU64,
    stopped: AtomicU64,
}

struct Inner {
    stage: Stage,
    done: bool,
    result: Option<Result<(), Error>>,
}

/// What [`LaunchSteps::apply`] is given: whether to stop, and where to count.
pub struct Walk<'a> {
    shared: &'a Shared,
    /// Whether the walk's bound has passed.
    over: &'a (dyn Fn() -> bool + Sync),
}

impl Walk<'_> {
    /// Whether the preparation was cancelled or its bound ([`PREPARE_BOUND`]) has passed: the
    /// walk stops at the next folder, with [`Error::Cancelled`] (the preparation then fails with
    /// the cancel, or with the timeout).
    pub fn cancelled(&self) -> bool {
        self.shared.is_cancelled() || (self.over)()
    }

    /// Counts `folders` more walked.
    pub fn walked(&self, folders: u64) {
        self.shared.walked.fetch_add(folders, Ordering::Relaxed);
    }
}

impl Shared {
    pub(super) fn new() -> Arc<Shared> {
        Arc::new(Shared {
            inner: Mutex::new(Inner {
                stage: Stage::Checking,
                done: false,
                result: None,
            }),
            finished: Condvar::new(),
            cancelled: AtomicBool::new(false),
            handles: AtomicUsize::new(0),
            walked: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
        })
    }

    /// Stops the walk at its next check; never blocks.
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Whether it was cancelled.
    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`Error::Cancelled`] once cancelled.
    fn go_on(&self) -> Result<(), Error> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    fn finish(&self, result: Result<(), Error>) {
        let mut inner = self.lock();
        inner.stage = Stage::Finished;
        inner.done = true;
        inner.result = Some(result);
        drop(inner);
        self.finished.notify_all();
    }
}

/// What a walk needs from the reservation, copied: it never holds the reservation itself.
pub(super) struct Reserved {
    pub(super) id: u64,
    pub(super) matter: String,
    pub(super) session: String,
    pub(super) pool: PoolUser,
    pub(super) slot: Slot,
}

/// The walk's look at its cancel and its bound, before and after each step.
struct Gate<'a, S: LaunchSteps> {
    steps: &'a S,
    shared: &'a Shared,
    /// When the walk started, on the steps' clock.
    started: Duration,
}

impl<S: LaunchSteps> Gate<'_, S> {
    /// Whether [`PREPARE_BOUND`] has passed since the walk started.
    fn over(&self) -> bool {
        self.steps.now().saturating_sub(self.started) >= PREPARE_BOUND
    }

    /// [`Error::Cancelled`] once cancelled; a timeout once the bound has passed.
    fn go_on(&self) -> Result<(), Error> {
        self.shared.go_on()?;
        if self.over() {
            tracing::warn!("tier B's preparation passed its bound");
            return Err(Error::Launch(clauseo_process::Error::Os {
                what: "the sandbox's preparation took longer than its bound",
                source: std::io::ErrorKind::TimedOut.into(),
            }));
        }
        Ok(())
    }

    /// Moves to `stage`, unless cancelled or past the bound.
    fn at(&self, stage: Stage) -> Result<(), Error> {
        self.shared.lock().stage = stage;
        self.go_on()
    }
}

impl Preparing {
    /// One finished already, with `result`.
    pub(super) fn finished(result: Result<(), Error>) -> Preparing {
        let shared = Shared::new();
        shared.finish(result);
        Preparing::joined(shared)
    }

    /// A handle on the walk `shared`, running: counted, so the last handle's drop cancels it.
    pub(super) fn joined(shared: Arc<Shared>) -> Preparing {
        shared.handles.fetch_add(1, Ordering::AcqRel);
        Preparing { shared }
    }

    /// Starts the walk `shared` for `reserved`, which the registry holds as preparing.
    pub(super) fn start<S: LaunchSteps>(
        registry: &Arc<Registry<S>>,
        reserved: Reserved,
        policy: Policy,
        shared: Arc<Shared>,
    ) -> Preparing {
        let ours = Arc::clone(&shared);
        let (matter, id) = (reserved.matter.clone(), reserved.id);
        let thread_registry = Arc::clone(registry);
        let started = std::thread::Builder::new()
            .name("clauseo-tierb-prepare".to_owned())
            .spawn(move || {
                // A panic in a step fails the walk, which then undoes and posts as any failure.
                let walked = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    walk(&thread_registry, &reserved, &policy, &ours)
                }))
                .unwrap_or_else(|_| {
                    tracing::warn!("tier B's preparation panicked in a step");
                    Err(Error::Launch(clauseo_process::Error::Os {
                        what: "the sandbox's preparation failed",
                        source: std::io::Error::other("a step panicked"),
                    }))
                });
                let folder: PathBuf = policy.matter().to_path_buf();
                // The undo runs here, before the result is posted.
                let result =
                    thread_registry.walk_done(&reserved.matter, reserved.id, walked, folder);
                ours.finish(result);
            });
        if let Err(error) = started {
            let error = Error::Launch(clauseo_process::Error::Os {
                what: "the sandbox's preparation didn't start",
                source: error,
            });
            let result = registry.walk_done(&matter, id, Err(error), PathBuf::new());
            shared.finish(result);
        }
        Preparing::joined(shared)
    }

    /// How far it is.
    pub fn progress(&self) -> Progress {
        let inner = self.shared.lock();
        Progress {
            stage: inner.stage,
            walked: self.shared.walked.load(Ordering::Relaxed),
            stopped: self.shared.stopped.load(Ordering::Relaxed),
            done: inner.done,
        }
    }

    /// Its result once finished: `Ok(())`, the reservation prepared for `launch` (every time
    /// it is asked), or why not, what was done for the session undone (the error once, then
    /// [`Error::NotPrepared`], to every handle of the walk). `None` while it runs.
    pub fn result(&self) -> Option<Result<(), Error>> {
        Self::take(&mut self.shared.lock())
    }

    /// [`Preparing::result`], waiting up to `timeout` for it to finish.
    pub fn wait(&self, timeout: Duration) -> Option<Result<(), Error>> {
        let inner = self.shared.lock();
        let (mut inner, _) = self
            .shared
            .finished
            .wait_timeout_while(inner, timeout, |inner| !inner.done)
            .unwrap_or_else(PoisonError::into_inner);
        Self::take(&mut inner)
    }

    fn take(inner: &mut Inner) -> Option<Result<(), Error>> {
        if !inner.done {
            return None;
        }
        match inner.result.take() {
            Some(Ok(())) => {
                inner.result = Some(Ok(()));
                Some(Ok(()))
            }
            Some(Err(error)) => Some(Err(error)),
            None => Some(Err(Error::NotPrepared)),
        }
    }

    /// Stops it at the next step, or the walk's next folder: it finishes with
    /// [`Error::Cancelled`], what was done for the session undone. On matter close or app quit.
    pub fn cancel(&self) {
        self.shared.cancel();
    }
}

impl Drop for Preparing {
    /// The last handle of a walk cancels it: nobody waits for it any more.
    fn drop(&mut self) {
        if self.shared.handles.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.shared.cancel();
        }
    }
}

impl std::fmt::Debug for Preparing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preparing")
            .field("progress", &self.progress())
            .finish()
    }
}

/// The walk's steps, in order; see the module's docs.
fn walk<S: LaunchSteps>(
    registry: &Registry<S>,
    reserved: &Reserved,
    policy: &Policy,
    shared: &Shared,
) -> Result<(), Error> {
    let steps = &*registry.steps;
    let gate = Gate {
        steps,
        shared,
        started: steps.now(),
    };
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
    let plan = steps.plan(policy, pool)?;
    gate.go_on()?;
    let check = steps.check_launch(&plan)?;
    gate.go_on()?;
    steps.check_network(policy, pool)?;
    gate.go_on()?;
    stop_leftovers(&gate, pool)?;
    gate.at(Stage::SettingAside)?;
    let moved = steps.before_launch(&plan, &check, session, &running)?;
    registry.update(matter, id, |entry| entry.moved = moved);
    gate.at(Stage::PreparingRun)?;
    let group = prepare_run(&gate, slot, pool)?;
    // The entry holds the group, so the undo ends it; with no entry, it is ended here.
    let mut group = Some(group);
    registry.update(matter, id, |entry| entry.run = group.take());
    if let Some(group) = group {
        steps.end_run(group);
        return Err(Error::WrongReservation);
    }
    gate.at(Stage::Walking)?;
    let over = || gate.over();
    let applied = steps.apply(
        &plan,
        &check,
        &Walk {
            shared,
            over: &over,
        },
    );
    // A cancel or the bound during `apply` is the walk's error, not `apply`'s own stop; the
    // registry looks at the cancel once more before it marks the claim prepared.
    gate.go_on()?;
    applied
}

/// Stops the pool user's leftover processes: the stop task, then its sweep, then a fresh look;
/// again every [`RETRY_PAUSE`] while any survives, until the walk's bound, past which the
/// launch is refused with [`Error::LeftoverProcess`].
fn stop_leftovers<S: LaunchSteps>(gate: &Gate<'_, S>, pool: &PoolUser) -> Result<(), Error> {
    let (steps, shared) = (gate.steps, gate.shared);
    let mut found = steps.pool_user_processes(pool)?;
    while !found.is_empty() {
        shared.go_on()?;
        if gate.over() {
            tracing::warn!(
                count = found.len(),
                "tier B's pool user kept a leftover process"
            );
            return Err(Error::LeftoverProcess { count: found.len() });
        }
        gate.at(Stage::Stopping)?;
        let Stopped { ended } = steps.stop_pool_user(pool)?;
        shared
            .stopped
            .fetch_add(u64::from(ended), Ordering::Relaxed);
        tracing::info!(
            found = found.len(),
            ended,
            "tier B stopped a pool user's leftovers"
        );
        gate.go_on()?;
        steps.sweep_pool_user(pool)?;
        gate.go_on()?;
        found = steps.pool_user_processes(pool)?;
        if !found.is_empty() {
            gate.go_on()?;
            steps.pause(RETRY_PAUSE);
        }
    }
    gate.go_on()
}

/// `PrepareRun`, tried again every [`RETRY_PAUSE`] while a set-up runs, until the walk's
/// bound; then a plain failure. A group it made is the caller's to record before it looks at
/// the bound again.
fn prepare_run<S: LaunchSteps>(
    gate: &Gate<'_, S>,
    slot: &Slot,
    pool: &PoolUser,
) -> Result<S::RunGroup, Error> {
    let (steps, shared) = (gate.steps, gate.shared);
    loop {
        match steps.prepare_run(slot, pool) {
            Ok(group) => return Ok(group),
            Err(RunRefusal::SetUpRunning) => {
                shared.go_on()?;
                if gate.over() {
                    tracing::warn!("tier B's preparation gave up waiting for a running set-up");
                    return Err(Error::Launch(clauseo_process::Error::Os {
                        what: "the sandbox's set-up is still running",
                        source: std::io::ErrorKind::TimedOut.into(),
                    }));
                }
                steps.pause(RETRY_PAUSE);
                shared.go_on()?;
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
