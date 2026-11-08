//! The matter's preparation, in the background: the walk `prepare` starts on a reservation, and
//! the handle the host polls ([`Preparing`]). The order, after `reserve`'s bookkeeping:
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
//! Nothing is granted to the pool user or run as it before the launch check passed: the sweep
//! (which runs as the pool user), `PrepareRun` (which adds it to the run's group) and `apply`
//! come after it. Steps 4 and 6 share one bound, [`PREPARE_BOUND`] from the walk's start.
//!
//! Any failure, and a cancel, drops the reservation, which undoes what was done for the session
//! (the group, the slot, the session's mark); the permission entries `apply` wrote stay in the
//! ledger, so the next preparation continues from them.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::registry::{Registry, State};
use super::steps::{LaunchSteps, RunRefusal, Slot, Stopped};
use crate::windows::pool::PoolUser;
use crate::{Blocked, Error, Policy, Reservation};

/// How long a preparation stops leftovers and retries `PrepareRun` while a set-up runs, from
/// the walk's start: the host's launch bound on tier B (0049). Past it, the preparation fails.
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

/// A matter's preparation, running in the background. Dropping it doesn't stop the walk (the
/// lawyer's Stop ends the wait, not the walk); the reservation is released when the walk ends.
/// [`Preparing::cancel`] stops it.
pub struct Preparing {
    shared: Arc<Shared>,
}

struct Shared {
    inner: Mutex<Inner>,
    finished: Condvar,
    cancelled: AtomicBool,
    walked: AtomicU64,
    stopped: AtomicU64,
}

struct Inner {
    stage: Stage,
    done: bool,
    result: Option<Result<Reservation, Error>>,
}

/// What [`LaunchSteps::apply`] is given: whether to stop, and where to count.
pub struct Walk<'a> {
    shared: &'a Shared,
}

impl Walk<'_> {
    /// Whether the preparation was cancelled: the walk stops at the next folder, with
    /// [`Error::Cancelled`].
    pub fn cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::Acquire)
    }

    /// Counts `folders` more walked.
    pub fn walked(&self, folders: u64) {
        self.shared.walked.fetch_add(folders, Ordering::Relaxed);
    }
}

impl Shared {
    fn new() -> Arc<Shared> {
        Arc::new(Shared {
            inner: Mutex::new(Inner {
                stage: Stage::Checking,
                done: false,
                result: None,
            }),
            finished: Condvar::new(),
            cancelled: AtomicBool::new(false),
            walked: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Moves to `stage`, unless cancelled.
    fn at(&self, stage: Stage) -> Result<(), Error> {
        self.lock().stage = stage;
        self.go_on()
    }

    /// [`Error::Cancelled`] once cancelled.
    fn go_on(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    fn finish(&self, result: Result<Reservation, Error>) {
        let mut inner = self.lock();
        inner.stage = Stage::Finished;
        inner.done = true;
        inner.result = Some(result);
        drop(inner);
        self.finished.notify_all();
    }
}

impl Preparing {
    /// One that failed before it started; `reservation`, if any, is dropped first.
    pub(super) fn failed(reservation: Option<Reservation>, error: Error) -> Preparing {
        drop(reservation);
        let shared = Shared::new();
        shared.finish(Err(error));
        Preparing { shared }
    }

    /// Starts the walk for claim `id`, which the registry holds as preparing.
    pub(super) fn start<S: LaunchSteps>(
        registry: Arc<Registry<S>>,
        id: u64,
        reservation: Reservation,
        policy: Policy,
    ) -> Preparing {
        let shared = Shared::new();
        let ours = Arc::clone(&shared);
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Reservation>(1);
        let started = std::thread::Builder::new()
            .name("clauseo-tierb-prepare".to_owned())
            .spawn(move || {
                let Ok(reservation) = receiver.recv() else {
                    return;
                };
                let walked = walk(&registry, id, &reservation, &policy, &ours);
                let result = walked.and_then(|()| {
                    let folder = policy.matter().to_path_buf();
                    let matter = reservation.matter();
                    let session = reservation.session();
                    let moved =
                        registry.step(matter, session, id, State::Preparing, State::Prepared);
                    registry.update(matter, id, |entry| entry.prepared = Some(folder));
                    if moved {
                        Ok(())
                    } else {
                        Err(Error::WrongReservation)
                    }
                });
                match result {
                    Ok(()) => ours.finish(Ok(reservation)),
                    Err(error) => {
                        // Undone before the host hears of the failure.
                        drop(reservation);
                        ours.finish(Err(error));
                    }
                }
            });
        match started {
            Ok(_) => {
                // The thread waits for it; it is dropped (released) if the thread has gone.
                let _sent = sender.send(reservation);
                Preparing { shared }
            }
            Err(error) => Preparing::failed(
                Some(reservation),
                Error::Launch(clauseo_process::Error::Os {
                    what: "the sandbox's preparation didn't start",
                    source: error,
                }),
            ),
        }
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

    /// Its result once finished, taken: the reservation, prepared, for `launch`, or why not
    /// (the reservation released). `None` while it runs, and once taken.
    pub fn result(&self) -> Option<Result<Reservation, Error>> {
        self.shared.lock().result.take()
    }

    /// [`Preparing::result`], waiting up to `timeout` for it to finish.
    pub fn wait(&self, timeout: Duration) -> Option<Result<Reservation, Error>> {
        let inner = self.shared.lock();
        let (mut inner, _) = self
            .shared
            .finished
            .wait_timeout_while(inner, timeout, |inner| !inner.done)
            .unwrap_or_else(PoisonError::into_inner);
        inner.result.take()
    }

    /// Stops it at the next step, or the walk's next folder: it finishes with
    /// [`Error::Cancelled`], what was done for the session undone. On matter close or app quit.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
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
    id: u64,
    reservation: &Reservation,
    policy: &Policy,
    shared: &Shared,
) -> Result<(), Error> {
    let steps = &*registry.steps;
    let started = steps.now();
    let matter = reservation.matter();
    let session = reservation.session();
    let (pool, slot) = match (reservation.pool_user(), reservation.slot()) {
        (Some(pool), Some(index)) => (
            pool,
            Slot {
                index,
                ports: reservation.ports().to_vec(),
            },
        ),
        _ => return Err(Error::WrongReservation),
    };
    let running = registry
        .update(matter, id, |entry| entry.running.clone())
        .ok_or(Error::WrongReservation)?;
    let running: Vec<&str> = running.iter().map(String::as_str).collect();
    shared.at(Stage::Checking)?;
    let plan = steps.plan(policy, pool)?;
    shared.go_on()?;
    let check = steps.check_launch(&plan)?;
    shared.go_on()?;
    steps.check_network(policy, pool)?;
    shared.go_on()?;
    stop_leftovers(steps, pool, started, shared)?;
    shared.at(Stage::SettingAside)?;
    let moved = steps.before_launch(&plan, &check, session, &running)?;
    registry.update(matter, id, |entry| entry.moved = moved);
    shared.at(Stage::PreparingRun)?;
    let group = prepare_run(steps, &slot, pool, started, shared)?;
    // The entry holds the group, so a release ends it; with no entry, it is ended here.
    let mut group = Some(group);
    registry.update(matter, id, |entry| entry.run = group.take());
    if let Some(group) = group {
        steps.end_run(group);
        return Err(Error::WrongReservation);
    }
    shared.at(Stage::Walking)?;
    steps.apply(&plan, &check, &Walk { shared })
}

/// Stops the pool user's leftover processes: the stop task, then its sweep, then a fresh look;
/// again every [`RETRY_PAUSE`] while any survives, until [`PREPARE_BOUND`] after `started`,
/// past which the launch is refused with [`Error::LeftoverProcess`].
fn stop_leftovers<S: LaunchSteps>(
    steps: &S,
    pool: &PoolUser,
    started: Duration,
    shared: &Shared,
) -> Result<(), Error> {
    let mut found = steps.pool_user_processes(pool)?;
    while !found.is_empty() {
        if steps.now().saturating_sub(started) >= PREPARE_BOUND {
            tracing::warn!(
                count = found.len(),
                "tier B's pool user kept a leftover process"
            );
            return Err(Error::LeftoverProcess { count: found.len() });
        }
        shared.at(Stage::Stopping)?;
        let Stopped { ended } = steps.stop_pool_user(pool)?;
        shared
            .stopped
            .fetch_add(u64::from(ended), Ordering::Relaxed);
        tracing::info!(
            found = found.len(),
            ended,
            "tier B stopped a pool user's leftovers"
        );
        steps.sweep_pool_user(pool)?;
        found = steps.pool_user_processes(pool)?;
        if !found.is_empty() {
            shared.go_on()?;
            steps.pause(RETRY_PAUSE);
        }
    }
    Ok(())
}

/// `PrepareRun`, tried again every [`RETRY_PAUSE`] while a set-up runs, until
/// [`PREPARE_BOUND`] after `started`; then a plain failure.
fn prepare_run<S: LaunchSteps>(
    steps: &S,
    slot: &Slot,
    pool: &PoolUser,
    started: Duration,
    shared: &Shared,
) -> Result<S::RunGroup, Error> {
    loop {
        match steps.prepare_run(slot, pool) {
            Ok(group) => return Ok(group),
            Err(RunRefusal::SetUpRunning) => {
                if steps.now().saturating_sub(started) >= PREPARE_BOUND {
                    tracing::warn!("tier B's preparation gave up waiting for a running set-up");
                    return Err(Error::Launch(clauseo_process::Error::Os {
                        what: "the sandbox's set-up is still running",
                        source: std::io::ErrorKind::TimedOut.into(),
                    }));
                }
                shared.go_on()?;
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
