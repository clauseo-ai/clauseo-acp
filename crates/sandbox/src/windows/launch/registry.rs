//! The session registry: which session holds each matter, from its reservation until its
//! after-session pass has finished. One live agent per matter on tier B (the orchestrator's
//! ruling): a matter in the registry refuses another reservation with
//! [`Error::MatterBusy`], so one walk runs per matter, and a matter's account is handed out
//! again only once the last session's job is gone and its after-session pass is done (and its
//! end sweeper, which `reserve` asks the steps about).
//!
//! **Nothing here blocks under the lock.** The registry's mutex is held only to read or change
//! the map: no step, file call or wait ever runs under it, and no walk holds it across one. So
//! a reservation's drop ([`Release::release`]) takes it briefly and never waits: it cancels a
//! walk (an atomic), forgets a failed claim, or hands the undo to a thread of its own.
//!
//! **A matter is freed only once nothing more runs for it.** A drop mid-walk marks the entry
//! releasing and cancels the walk; the walk's own thread undoes and frees it when it stops, so
//! a new `reserve` is `MatterBusy` until then and never gets the pool user while the old walk
//! could still stop, prepare or apply. Every undo and every end frees the matter through a
//! drop guard ([`Finish`]), so a panic in a step never leaves it held.
//!
//! **A step that never returns** can't be undone around (it could still grant once it returns):
//! past its limit and [`STUCK_GRACE`](super::STUCK_GRACE), `reserve` marks the matter stuck and
//! refuses it with [`Error::MatterStuck`] until Clauseo restarts (`registry/held.rs`), or, for
//! a session's end, until its steps return. A walk and a session's end are stuck by the same
//! progress rule (no counted progress for the stall timer, or past the cap, plus the grace).
//! A failed `EndRun` keeps the pool user and its slot held until a later `EndRun` succeeds.
//!
//! **The map is this process's;** the sandbox folder's lock (`tier-b-launchers.lock`, held as
//! long as the registry lives) keeps any other set of launchers, here or in another Clauseo,
//! from starting over the same folder and clearing its live marks (`Launchers::with_steps`).

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::prepare::Shared;
use super::steps::{LaunchSteps, Slot};
use crate::reservation::Release;
use crate::windows::pool::PoolUser;
use crate::{Error, SetAside};

mod ended;
mod held;

pub(super) use ended::{Ended, OuterState, finish_race};
pub(super) use held::{Finish, FinishEnd};
use held::{Owed, stuck};

/// Each registry's own number, so a reservation another set of launchers handed out is told
/// apart.
static OWNERS: AtomicU64 = AtomicU64::new(1);
/// Claim ids, unique in the process, so a slot record names the claim that made it.
static CLAIMS: AtomicU64 = AtomicU64::new(1);

/// Where a matter's session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// `reserve` is running.
    Reserving,
    /// Reserved: the host holds the reservation.
    Reserved,
    /// Its walk runs (`prepare`).
    Preparing,
    /// Prepared, for `launch`.
    Prepared,
    /// Its walk failed or was cancelled, and undid what was done: `launch` refuses it, and
    /// the matter stays held until the host drops the reservation.
    Failed,
    /// Launched: the agent may run, until `after_session`.
    Launched,
    /// `after_session` is running.
    Ending,
    /// Its reservation (or its unstarted launcher) was dropped: a walk still running for it, or
    /// a release thread, is undoing what was done, and frees the matter when done. Busy till then.
    Releasing,
    /// Its walk failed and is undoing what it did: busy until the undo ends, then failed (or
    /// freed, if the reservation was dropped meanwhile: releasing).
    Undoing,
    /// A step for it didn't return within its limit and grace: refused with
    /// [`Error::MatterStuck`] until Clauseo restarts; never freed in this process, but for a
    /// session's end whose steps return at last (`registry/held.rs`'s `finish_end`).
    Stuck,
}

/// One matter's session, and what was done for it, so a release undoes just that.
pub(super) struct Entry<S: LaunchSteps> {
    pub(super) id: u64,
    pub(super) session: String,
    pub(super) state: State,
    /// The other sessions of the matter the host still runs, as `reserve` was given them.
    pub(super) running: Vec<String>,
    /// `open_session` marked the session.
    pub(super) opened: bool,
    pub(super) pool: Option<PoolUser>,
    pub(super) slot: Option<Slot>,
    /// `PrepareRun`'s group, which `EndRun` deletes.
    pub(super) run: Option<S::RunGroup>,
    /// What the set-aside's launch pass moved, for `launch` to return.
    pub(super) moved: Vec<SetAside>,
    /// The matter folder the walk prepared, which `launch`'s policy must name.
    pub(super) prepared: Option<PathBuf>,
    /// The running walk, which a drop of the reservation cancels and a second `prepare` joins.
    /// Releasing with a walk: the walk undoes and frees the matter when it stops. Ending: the
    /// end's progress, which its stall rule reads.
    pub(super) walk: Option<Arc<Shared>>,
    /// The state a step hung in, once `begin` marked it stuck (not a launcher's `stick`): a
    /// session's end stuck this way is freed once its steps return (`finish_end`).
    pub(super) stuck_from: Option<State>,
    /// When it entered its state, on the steps' clock: the limit of a reserve, an undo, a
    /// release or a session's end counts from here.
    pub(super) since: Duration,
    /// Where the launcher's start of the outer is (`ended.rs`).
    pub(super) outer: OuterState,
    /// The launch file, until the outer has read it: deleted by the feed then, else by the
    /// session's end or undo.
    pub(super) launch_file: Option<PathBuf>,
}

/// What an undo takes from an entry: the run's group, the slot, the session's mark.
pub(super) struct Undo<S: LaunchSteps> {
    matter: String,
    session: String,
    pool: Option<PoolUser>,
    run: Option<S::RunGroup>,
    slot: Option<Slot>,
    opened: bool,
    launch_file: Option<PathBuf>,
}

/// What follows a walk's undo: the matter freed (the reservation was dropped), kept stuck, or
/// settled (failed, or freed if dropped during the undo).
enum After {
    Free,
    Stuck,
    Settle,
}

/// What `prepare` found for a reservation.
pub(super) enum Begun {
    /// A walk to start, its state made.
    Start(Arc<Shared>),
    /// A walk running already, to join.
    Join(Arc<Shared>),
    /// Prepared already.
    Done,
}

/// The sessions of one set of launchers, by matter.
pub(super) struct Registry<S: LaunchSteps> {
    pub(super) owner: u64,
    pub(super) steps: Arc<S>,
    matters: Mutex<BTreeMap<String, Entry<S>>>,
    /// The run groups whose `EndRun` failed, with their pool users and slots, held back until
    /// a later `EndRun` succeeds.
    owed: Mutex<Vec<Owed<S>>>,
    /// Signalled whenever a matter is freed.
    freed: Condvar,
    /// The sandbox folder's lock, held while anything (a reservation, a walk, a release thread)
    /// can still use the registry. `None` only once a test let go of it as a crash would.
    data_lock: Mutex<Option<File>>,
    /// The ends dropped launchers run on threads of their own (`end.rs`).
    pub(super) ends: super::end::Ends,
    /// Launch files whose removal failed, tried again at each end and undo.
    pub(super) files: super::end::LaunchFiles,
}

impl<S: LaunchSteps> Registry<S> {
    /// The registry over `steps`, holding the sandbox folder's lock `data_lock`.
    pub(super) fn new(steps: Arc<S>, data_lock: File) -> Registry<S> {
        Registry {
            owner: OWNERS.fetch_add(1, Ordering::Relaxed),
            steps,
            matters: Mutex::new(BTreeMap::new()),
            owed: Mutex::new(Vec::new()),
            freed: Condvar::new(),
            data_lock: Mutex::new(Some(data_lock)),
            ends: super::end::Ends::default(),
            files: super::end::LaunchFiles::default(),
        }
    }

    /// [`held::end_run_and_free_slot`].
    pub(super) fn end_run_and_free_slot(
        &self,
        pool: Option<&PoolUser>,
        run: Option<S::RunGroup>,
        slot: Option<Slot>,
    ) {
        held::end_run_and_free_slot(self, pool, run, slot);
    }

    /// [`held::retry_owed`].
    pub(super) fn retry_owed(&self, pool: &PoolUser) -> Result<(), Error> {
        held::retry_owed(self, pool)
    }

    /// Whether claim `id` is launched, its end not begun: what a launcher checks before it
    /// starts the outer.
    pub(super) fn is_launched(&self, id: u64) -> bool {
        self.lock()
            .values()
            .any(|entry| entry.id == id && entry.state == State::Launched)
    }

    /// Marks claim `id`'s matter stuck: a launcher that couldn't end the outer it may have
    /// started.
    pub(super) fn stick(&self, id: u64) {
        if let Some(entry) = self.lock().values_mut().find(|entry| entry.id == id) {
            entry.state = State::Stuck;
        }
    }

    /// Lets go of the sandbox folder's lock, as a crash would.
    #[cfg(test)]
    pub(super) fn crash(&self) {
        self.data_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Entry<S>>> {
        self.matters.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes `matter` for `session`: [`Error::MatterBusy`] while any session holds it, or
    /// [`Error::MatterStuck`] once a step for it has hung past its limit ([`Registry::stuck`]).
    pub(super) fn begin(
        &self,
        matter: &str,
        session: &str,
        running: &[&str],
    ) -> Result<u64, Error> {
        let now = self.steps.now();
        let mut matters = self.lock();
        if let Some(entry) = matters.get_mut(matter) {
            if stuck(entry, now) {
                if entry.state != State::Stuck {
                    tracing::warn!(state = ?entry.state, "tier B's matter is stuck in a step");
                    entry.stuck_from = Some(entry.state);
                    entry.state = State::Stuck;
                }
                return Err(Error::MatterStuck);
            }
            return Err(Error::MatterBusy);
        }
        let id = CLAIMS.fetch_add(1, Ordering::Relaxed);
        matters.insert(
            matter.to_owned(),
            Entry {
                id,
                session: session.to_owned(),
                state: State::Reserving,
                running: running.iter().map(|&other| other.to_owned()).collect(),
                opened: false,
                pool: None,
                slot: None,
                run: None,
                moved: Vec::new(),
                prepared: None,
                walk: None,
                stuck_from: None,
                since: now,
                outer: OuterState::Unstarted,
                launch_file: None,
            },
        );
        Ok(id)
    }

    /// Changes the entry of claim `id` of `matter`; `None` when there is none.
    pub(super) fn update<T>(
        &self,
        matter: &str,
        id: u64,
        change: impl FnOnce(&mut Entry<S>) -> T,
    ) -> Option<T> {
        let mut matters = self.lock();
        matters
            .get_mut(matter)
            .filter(|entry| entry.id == id)
            .map(change)
    }

    /// Starts or joins the walk of claim `id`, for `session` of `matter`: the claim and the
    /// session together name the launching session. A failed walk's claim is
    /// [`Error::NotPrepared`] (the host drops it and reserves again); anything else but reserved,
    /// preparing or prepared is [`Error::WrongReservation`].
    pub(super) fn begin_walk(&self, matter: &str, session: &str, id: u64) -> Result<Begun, Error> {
        let now = self.steps.now();
        let mut matters = self.lock();
        let entry = matters
            .get_mut(matter)
            .filter(|entry| entry.id == id && entry.session == session)
            .ok_or(Error::WrongReservation)?;
        match entry.state {
            State::Reserved => {
                let shared = Shared::new(now);
                entry.state = State::Preparing;
                entry.since = now;
                entry.walk = Some(Arc::clone(&shared));
                Ok(Begun::Start(shared))
            }
            State::Preparing => entry
                .walk
                .as_ref()
                .map(|walk| Begun::Join(Arc::clone(walk)))
                .ok_or(Error::WrongReservation),
            State::Prepared => Ok(Begun::Done),
            State::Failed | State::Undoing => Err(Error::NotPrepared),
            State::Stuck => Err(Error::MatterStuck),
            State::Reserving | State::Launched | State::Ending | State::Releasing => {
                Err(Error::WrongReservation)
            }
        }
    }

    /// The walk of claim `id` ended with `walked`, on the matter folder `folder`. Prepared on
    /// success, unless the walk was cancelled meanwhile (looked at once more here, under the
    /// lock, so a cancel that came after `apply` is never prepared); otherwise the undo runs
    /// here, on the walk's thread, the claim undoing until it ends, then failed, or, when the
    /// reservation was dropped meanwhile (releasing), the matter is freed ([`Error::Cancelled`]).
    /// A stuck one is undone too (its step returned at last) but stays stuck. What the walk's
    /// handles are told.
    pub(super) fn walk_done(
        &self,
        matter: &str,
        id: u64,
        walked: Result<(), Error>,
        folder: PathBuf,
    ) -> Result<(), Error> {
        let mut walked = walked;
        let now = self.steps.now();
        let (undo, after) = {
            let mut matters = self.lock();
            let Some(entry) = matters.get_mut(matter).filter(|entry| entry.id == id) else {
                return Err(Error::WrongReservation);
            };
            let walk = entry.walk.take();
            if walked.is_ok() && walk.is_some_and(|walk| walk.is_cancelled()) {
                walked = Err(Error::Cancelled);
            }
            if entry.state == State::Releasing {
                (Some(take_undo(matter, entry)), After::Free)
            } else if entry.state == State::Stuck {
                (Some(take_undo(matter, entry)), After::Stuck)
            } else if walked.is_ok() {
                entry.state = State::Prepared;
                entry.prepared = Some(folder);
                (None, After::Settle)
            } else {
                entry.state = State::Undoing;
                entry.since = now;
                (Some(take_undo(matter, entry)), After::Settle)
            }
        };
        let Some(undo) = undo else {
            return walked;
        };
        match after {
            After::Free => {
                self.undo_and_finish(id, undo);
                Err(Error::Cancelled)
            }
            After::Stuck => {
                self.undo(undo);
                Err(Error::MatterStuck)
            }
            After::Settle => {
                self.undo(undo);
                self.undone(matter, id);
                walked
            }
        }
    }

    /// The failed walk of claim `id` has undone its work: failed, for the host to drop; or, if
    /// the reservation was dropped meanwhile, the matter freed.
    fn undone(&self, matter: &str, id: u64) {
        let mut matters = self.lock();
        let Some(entry) = matters.get_mut(matter).filter(|entry| entry.id == id) else {
            return;
        };
        if entry.state == State::Undoing {
            entry.state = State::Failed;
        } else if entry.state == State::Releasing {
            matters.remove(matter);
            self.freed.notify_all();
        }
    }

    /// Claim `id`'s account, if `launch` would take it ([`Registry::launch`]'s checks), with
    /// nothing changed.
    pub(super) fn prepared(
        &self,
        matter: &str,
        session: &str,
        id: u64,
        folder: &Path,
    ) -> Result<PoolUser, Error> {
        let matters = self.lock();
        let entry = matters
            .get(matter)
            .filter(|entry| entry.id == id && entry.session == session)
            .ok_or(Error::WrongReservation)?;
        launchable(entry, folder)?;
        entry.pool.clone().ok_or(Error::WrongReservation)
    }

    /// Launches claim `id`: prepared, for `session`, on the matter folder `folder`. Its account
    /// and what the launch pass moved. A reserved one is [`Error::NotPrepared`]; anything else
    /// [`Error::WrongReservation`].
    pub(super) fn launch(
        &self,
        matter: &str,
        session: &str,
        id: u64,
        folder: &Path,
    ) -> Result<(PoolUser, Vec<SetAside>), Error> {
        let mut matters = self.lock();
        let Some(entry) = matters
            .get_mut(matter)
            .filter(|entry| entry.id == id && entry.session == session)
        else {
            return Err(Error::WrongReservation);
        };
        launchable(entry, folder)?;
        let pool = entry.pool.clone().ok_or(Error::WrongReservation)?;
        entry.state = State::Launched;
        Ok((pool, std::mem::take(&mut entry.moved)))
    }

    /// Starts `session`'s end, once (`ended.rs`).
    pub(super) fn end(&self, matter: &str, session: &str, id: Option<u64>) -> Option<Ended<S>> {
        ended::end(self, matter, session, id)
    }

    /// Runs `undo`'s steps, then frees claim `id`'s matter, even if a step panics. Blocks:
    /// never under the lock, and never from a drop.
    fn undo_and_finish(&self, id: u64, undo: Undo<S>) {
        let matter = undo.matter.clone();
        let _finish = Finish {
            registry: self,
            matter: &matter,
            id,
        };
        self.undo(undo);
    }

    /// Runs `undo`'s steps, newest first: `EndRun`, the slot, the session's mark. Blocks:
    /// never under the lock, and never from a drop.
    fn undo(&self, undo: Undo<S>) {
        if let Some(file) = &undo.launch_file {
            self.files.remove(&*self.steps, file);
        }
        self.files.retry(&*self.steps);
        self.end_run_and_free_slot(undo.pool.as_ref(), undo.run, undo.slot);
        if undo.opened {
            self.steps.close_session(&undo.matter, &undo.session);
        }
    }

    /// Lets go of claim `id`, never blocking: a running walk is cancelled and the entry marked
    /// releasing (the walk undoes and frees the matter when it stops), a failed claim is
    /// forgotten, and anything else not yet ended is marked releasing, its undo handed back for
    /// the caller to run, then [`Finish`]. Launched counts: a launcher's claim is
    /// armed only until its agent starts (`tier_b.rs`); once `after_session` began (ending), its
    /// end is that pass's.
    fn detach(&self, id: u64) -> Option<(u64, Undo<S>)> {
        let now = self.steps.now();
        let mut matters = self.lock();
        let (matter, entry) = matters.iter_mut().find(|(_, entry)| entry.id == id)?;
        match entry.state {
            State::Preparing => {
                entry.state = State::Releasing;
                if let Some(walk) = &entry.walk {
                    walk.cancel();
                }
                None
            }
            State::Failed => {
                let matter = matter.clone();
                matters.remove(&matter);
                self.freed.notify_all();
                None
            }
            State::Undoing => {
                // The walk's thread frees the matter once its undo ends.
                entry.state = State::Releasing;
                None
            }
            State::Reserving | State::Reserved | State::Prepared | State::Launched => {
                entry.state = State::Releasing;
                entry.since = now;
                Some((id, take_undo(&matter.clone(), entry)))
            }
            State::Ending | State::Releasing | State::Stuck => None,
        }
    }

    /// Lets go of claim `id` and runs its undo here: `reserve`'s and `launch`'s refusals, and
    /// a launcher whose agent didn't start, which may block.
    pub(super) fn release_now(&self, id: u64) {
        if let Some((id, undo)) = self.detach(id) {
            self.undo_and_finish(id, undo);
        }
    }

    /// Waits up to `timeout` for `matter` to be free; whether it is.
    #[cfg(test)]
    pub(super) fn wait_free(&self, matter: &str, timeout: std::time::Duration) -> bool {
        let matters = self.lock();
        let (matters, _) = self
            .freed
            .wait_timeout_while(matters, timeout, |matters| matters.contains_key(matter))
            .unwrap_or_else(PoisonError::into_inner);
        !matters.contains_key(matter)
    }
}

impl<S: LaunchSteps> Drop for Registry<S> {
    /// The last user is gone (the launchers, every reservation, walk and release thread): the
    /// sandbox folder's lock goes, so the next launchers may start.
    fn drop(&mut self) {
        let lock = self
            .data_lock
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if lock.take().is_some() {
            tracing::debug!("tier B's launchers let go of the sandbox folder's lock");
        }
    }
}

/// Takes what `entry`'s undo needs, so it runs once.
fn take_undo<S: LaunchSteps>(matter: &str, entry: &mut Entry<S>) -> Undo<S> {
    Undo {
        matter: matter.to_owned(),
        session: entry.session.clone(),
        pool: entry.pool.clone(),
        run: entry.run.take(),
        slot: entry.slot.take(),
        opened: std::mem::take(&mut entry.opened),
        launch_file: entry.launch_file.take(),
    }
}

/// Whether `entry` can be launched on the matter folder `folder`: prepared, for it. Reserved, or
/// a failed walk's, is [`Error::NotPrepared`]; anything else [`Error::WrongReservation`].
fn launchable<S: LaunchSteps>(entry: &Entry<S>, folder: &Path) -> Result<(), Error> {
    match entry.state {
        State::Prepared if entry.prepared.as_deref() == Some(folder) => Ok(()),
        State::Reserving | State::Reserved | State::Failed | State::Undoing => {
            Err(Error::NotPrepared)
        }
        State::Stuck => Err(Error::MatterStuck),
        State::Prepared | State::Preparing | State::Launched | State::Ending | State::Releasing => {
            Err(Error::WrongReservation)
        }
    }
}

impl<S: LaunchSteps> Release for Registry<S> {
    /// The reservation's drop: never blocks ([`Registry::detach`]). An undo with steps to run
    /// goes to a thread of its own, which frees the matter when done; the matter stays busy
    /// (`MatterBusy`) until then. Only if no thread can be started does it run here.
    fn release(self: Arc<Self>, id: u64) {
        let Some((id, undo)) = self.detach(id) else {
            return;
        };
        let registry = Arc::clone(&self);
        let (sender, receiver) = std::sync::mpsc::channel::<Undo<S>>();
        let started = std::thread::Builder::new()
            .name("clauseo-tierb-release".to_owned())
            .spawn(move || {
                if let Ok(undo) = receiver.recv() {
                    registry.undo_and_finish(id, undo);
                }
            });
        if started.is_ok() {
            if let Err(unsent) = sender.send(undo) {
                // The thread is gone already: the undo runs here after all.
                self.undo_and_finish(id, unsent.0);
            }
        } else {
            tracing::warn!("tier B's release ran on the dropping thread: no thread started");
            self.undo_and_finish(id, undo);
        }
    }
}
