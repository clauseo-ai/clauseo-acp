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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use super::prepare::Shared;
use super::steps::{LaunchSteps, Slot};
use crate::reservation::Release;
use crate::windows::pool::PoolUser;
use crate::{Error, SetAside};

/// Each registry's own number, so a reservation another set of launchers handed out is told
/// apart.
static OWNERS: AtomicU64 = AtomicU64::new(1);

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
    /// A release is undoing what was done for it: busy until that's done.
    Releasing,
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
    pub(super) walk: Option<Arc<Shared>>,
    /// The reservation was dropped while its walk ran: the walk undoes and frees the matter.
    pub(super) dropped: bool,
}

/// What an undo takes from an entry: the run's group, the slot, the session's mark.
pub(super) struct Undo<S: LaunchSteps> {
    matter: String,
    session: String,
    run: Option<S::RunGroup>,
    slot: Option<Slot>,
    opened: bool,
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

/// What `after_session` takes from a launched session's entry.
pub(super) struct Ended<S: LaunchSteps> {
    pub(super) id: u64,
    pub(super) pool: Option<PoolUser>,
    pub(super) slot: Option<Slot>,
    pub(super) run: Option<S::RunGroup>,
}

/// The sessions of one set of launchers, by matter.
pub(super) struct Registry<S: LaunchSteps> {
    pub(super) owner: u64,
    pub(super) steps: Arc<S>,
    next: AtomicU64,
    matters: Mutex<BTreeMap<String, Entry<S>>>,
    /// Signalled whenever a matter is freed.
    freed: Condvar,
}

impl<S: LaunchSteps> Registry<S> {
    pub(super) fn new(steps: Arc<S>) -> Registry<S> {
        Registry {
            owner: OWNERS.fetch_add(1, Ordering::Relaxed),
            steps,
            next: AtomicU64::new(1),
            matters: Mutex::new(BTreeMap::new()),
            freed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Entry<S>>> {
        self.matters.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes `matter` for `session`: [`Error::MatterBusy`] while any session holds it.
    pub(super) fn begin(
        &self,
        matter: &str,
        session: &str,
        running: &[&str],
    ) -> Result<u64, Error> {
        let mut matters = self.lock();
        if matters.contains_key(matter) {
            return Err(Error::MatterBusy);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
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
                dropped: false,
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
        let mut matters = self.lock();
        let entry = matters
            .get_mut(matter)
            .filter(|entry| entry.id == id && entry.session == session && !entry.dropped)
            .ok_or(Error::WrongReservation)?;
        match entry.state {
            State::Reserved => {
                let shared = Shared::new();
                entry.state = State::Preparing;
                entry.walk = Some(Arc::clone(&shared));
                Ok(Begun::Start(shared))
            }
            State::Preparing => entry
                .walk
                .as_ref()
                .map(|walk| Begun::Join(Arc::clone(walk)))
                .ok_or(Error::WrongReservation),
            State::Prepared => Ok(Begun::Done),
            State::Failed => Err(Error::NotPrepared),
            State::Reserving | State::Launched | State::Ending | State::Releasing => {
                Err(Error::WrongReservation)
            }
        }
    }

    /// The walk of claim `id` ended with `walked`, on the matter folder `folder`. Prepared on
    /// success; otherwise the undo runs here, on the walk's thread, and the claim is failed, or,
    /// when the reservation was dropped meanwhile, the matter is freed ([`Error::Cancelled`]).
    /// What the walk's handles are told.
    pub(super) fn walk_done(
        &self,
        matter: &str,
        id: u64,
        walked: Result<(), Error>,
        folder: PathBuf,
    ) -> Result<(), Error> {
        let (undo, dropped) = {
            let mut matters = self.lock();
            let Some(entry) = matters.get_mut(matter).filter(|entry| entry.id == id) else {
                return Err(Error::WrongReservation);
            };
            entry.walk = None;
            if entry.dropped {
                entry.state = State::Releasing;
                (Some(take_undo(matter, entry)), true)
            } else if walked.is_ok() {
                entry.state = State::Prepared;
                entry.prepared = Some(folder);
                (None, false)
            } else {
                entry.state = State::Failed;
                (Some(take_undo(matter, entry)), false)
            }
        };
        if let Some(undo) = undo {
            self.undo(undo);
        }
        if dropped {
            self.finish(matter, id);
            return Err(Error::Cancelled);
        }
        walked
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

    /// Starts `session`'s end: launched, it becomes ending. `None` for a session not launched.
    pub(super) fn end(&self, matter: &str, session: &str) -> Option<Ended<S>> {
        let mut matters = self.lock();
        let entry = matters
            .get_mut(matter)
            .filter(|entry| entry.session == session && entry.state == State::Launched)?;
        entry.state = State::Ending;
        Some(Ended {
            id: entry.id,
            pool: entry.pool.clone(),
            slot: entry.slot.clone(),
            run: entry.run.take(),
        })
    }

    /// Frees `matter` once claim `id`'s session has ended.
    pub(super) fn finish(&self, matter: &str, id: u64) {
        let mut matters = self.lock();
        if matters.get(matter).is_some_and(|entry| entry.id == id) {
            matters.remove(matter);
            self.freed.notify_all();
        }
    }

    /// Runs `undo`'s steps, newest first: `EndRun`, the slot, the session's mark. Blocks:
    /// never under the lock, and never from a drop.
    fn undo(&self, undo: Undo<S>) {
        if let Some(run) = undo.run {
            self.steps.end_run(run);
        }
        if let Some(slot) = &undo.slot {
            self.steps.release_slot(slot);
        }
        if undo.opened {
            self.steps.close_session(&undo.matter, &undo.session);
        }
    }

    /// Lets go of claim `id`, never blocking: a running walk is cancelled (it undoes and frees
    /// the matter when it stops), a failed claim is forgotten, and anything else is marked
    /// releasing, its undo handed back for the caller to run, then [`Registry::finish`].
    fn detach(&self, id: u64) -> Option<(u64, Undo<S>)> {
        let mut matters = self.lock();
        let (matter, entry) = matters.iter_mut().find(|(_, entry)| entry.id == id)?;
        match entry.state {
            State::Preparing => {
                entry.dropped = true;
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
            State::Reserving | State::Reserved | State::Prepared => {
                entry.state = State::Releasing;
                Some((id, take_undo(&matter.clone(), entry)))
            }
            State::Launched | State::Ending | State::Releasing => None,
        }
    }

    /// Lets go of claim `id` and runs its undo here: `reserve`'s and `launch`'s refusals,
    /// which may block.
    pub(super) fn release_now(&self, id: u64) {
        if let Some((id, undo)) = self.detach(id) {
            let matter = undo.matter.clone();
            self.undo(undo);
            self.finish(&matter, id);
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

/// Takes what `entry`'s undo needs, so it runs once.
fn take_undo<S: LaunchSteps>(matter: &str, entry: &mut Entry<S>) -> Undo<S> {
    Undo {
        matter: matter.to_owned(),
        session: entry.session.clone(),
        run: entry.run.take(),
        slot: entry.slot.take(),
        opened: std::mem::take(&mut entry.opened),
    }
}

/// Whether `entry` can be launched on the matter folder `folder`: prepared, for it. Reserved, or
/// a failed walk's, is [`Error::NotPrepared`]; anything else [`Error::WrongReservation`].
fn launchable<S: LaunchSteps>(entry: &Entry<S>, folder: &Path) -> Result<(), Error> {
    match entry.state {
        State::Prepared if entry.prepared.as_deref() == Some(folder) => Ok(()),
        State::Reserving | State::Reserved | State::Failed => Err(Error::NotPrepared),
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
                    let matter = undo.matter.clone();
                    registry.undo(undo);
                    registry.finish(&matter, id);
                }
            });
        if started.is_ok() {
            if let Err(unsent) = sender.send(undo) {
                // The thread is gone already: the undo runs here after all.
                let undo = unsent.0;
                let matter = undo.matter.clone();
                self.undo(undo);
                self.finish(&matter, id);
            }
        } else {
            tracing::warn!("tier B's release ran on the dropping thread: no thread started");
            let matter = undo.matter.clone();
            self.undo(undo);
            self.finish(&matter, id);
        }
    }
}
