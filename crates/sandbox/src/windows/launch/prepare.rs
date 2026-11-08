//! The matter's preparation, in the background: the handle the host polls ([`Preparing`]) and
//! the state its walk shares with it. The walk itself, its order and its limits, is `walk.rs`.
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
use super::steps::{LaunchSteps, Slot};
use super::walk::walk;
use crate::windows::pool::PoolUser;
use crate::{Error, Policy};

/// The budget the walk's **waits** share, on a wall clock: the leftover stops, sweeps and
/// re-checks and the `PrepareRun` retries while a set-up runs. Running out refuses the launch
/// (`LeftoverProcess`, or the set-up's words). The work steps have no such cap ([`PREPARE_STALL`]).
pub const PREPARE_BOUND: Duration = Duration::from_secs(180);
/// How long it waits between those tries, and before it stops a leftover again.
pub const RETRY_PAUSE: Duration = Duration::from_secs(1);
/// The **work** steps (the plan, the launch check's scan, the set-aside's pass, `apply`) take as
/// long as the matter's size needs, but no longer than this without progress: a walk whose
/// progress doesn't move for this long cancels, undoes and fails with
/// [`Error::PrepareStalled`]. The real steps tick [`Walk::tick`] every few seconds.
pub const PREPARE_STALL: Duration = Duration::from_secs(60);
/// How long past its limit a step that hasn't returned may take before its matter is
/// [`Error::MatterStuck`] (until Clauseo restarts): a walk with no progress for
/// [`PREPARE_STALL`] plus this, or an undo, a release or a session's end running for
/// [`PREPARE_BOUND`] plus this.
pub const STUCK_GRACE: Duration = Duration::from_secs(60);

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
    /// When its progress last moved, on the steps' clock.
    progressed: Duration,
}

/// What the work steps ([`LaunchSteps::plan`], [`LaunchSteps::check_launch`],
/// [`LaunchSteps::before_launch`], [`LaunchSteps::apply`]) are given: whether to stop, and
/// where to report progress.
pub struct Walk<'a> {
    pub(super) shared: &'a Shared,
    /// The steps' clock.
    pub(super) now: &'a (dyn Fn() -> Duration + Sync),
}

impl Walk<'_> {
    /// Whether the preparation was cancelled or stalled (no progress for [`PREPARE_STALL`]):
    /// the step stops at its next folder or item, with [`Error::Cancelled`] (the preparation
    /// then fails with the cancel, or with [`Error::PrepareStalled`]).
    pub fn cancelled(&self) -> bool {
        self.shared.is_cancelled() || self.shared.stalled((self.now)())
    }

    /// Counts `folders` more walked: progress.
    pub fn walked(&self, folders: u64) {
        self.shared.walked.fetch_add(folders, Ordering::Relaxed);
        self.tick();
    }

    /// Reports progress with nothing to count (a scan's files, a pass's items): the real steps
    /// tick at least every few seconds, or [`PREPARE_STALL`] cancels the walk.
    pub fn tick(&self) {
        self.shared.tick((self.now)());
    }
}

impl Shared {
    /// A walk's state, its progress last moved at `now`.
    pub(super) fn new(now: Duration) -> Arc<Shared> {
        Arc::new(Shared {
            inner: Mutex::new(Inner {
                stage: Stage::Checking,
                done: false,
                result: None,
                progressed: now,
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

    /// Its progress moved at `now`.
    pub(super) fn tick(&self, now: Duration) {
        self.lock().progressed = now;
    }

    /// How long, at `now`, since its progress last moved.
    pub(super) fn idle(&self, now: Duration) -> Duration {
        now.saturating_sub(self.lock().progressed)
    }

    /// Whether, at `now`, its progress hasn't moved for [`PREPARE_STALL`].
    pub(super) fn stalled(&self, now: Duration) -> bool {
        self.idle(now) >= PREPARE_STALL
    }

    /// Moves to `stage`.
    pub(super) fn set_stage(&self, stage: Stage) {
        self.lock().stage = stage;
    }

    /// Counts `ended` more leftovers stopped.
    pub(super) fn stopped(&self, ended: u32) {
        self.stopped.fetch_add(u64::from(ended), Ordering::Relaxed);
    }

    /// [`Error::Cancelled`] once cancelled.
    pub(super) fn go_on(&self) -> Result<(), Error> {
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

impl Preparing {
    /// One finished already, with `result`.
    pub(super) fn finished(result: Result<(), Error>) -> Preparing {
        let shared = Shared::new(Duration::ZERO);
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
