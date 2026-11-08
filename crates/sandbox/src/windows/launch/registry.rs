//! The session registry: which session holds each matter, from its reservation until its
//! after-session pass has finished. One live agent per matter on tier B (the orchestrator's
//! ruling): a matter in the registry refuses another reservation with
//! [`Error::MatterBusy`], so one walk runs per matter, and a matter's account is handed out
//! again only once the last session's job is gone and its after-session pass is done.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
    /// The walk holds it (`prepare`).
    Preparing,
    /// Prepared: the host holds it again, for `launch`.
    Prepared,
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
}

impl<S: LaunchSteps> Registry<S> {
    pub(super) fn new(steps: Arc<S>) -> Registry<S> {
        Registry {
            owner: OWNERS.fetch_add(1, Ordering::Relaxed),
            steps,
            next: AtomicU64::new(1),
            matters: Mutex::new(BTreeMap::new()),
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

    /// Moves claim `id` of `matter` and `session` from `from` to `to`; whether it was there.
    pub(super) fn step(
        &self,
        matter: &str,
        session: &str,
        id: u64,
        from: State,
        to: State,
    ) -> bool {
        self.update(matter, id, |entry| {
            let at = entry.session == session && entry.state == from;
            if at {
                entry.state = to;
            }
            at
        })
        .unwrap_or(false)
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
        }
    }
}

/// Whether `entry` can be launched on the matter folder `folder`: prepared, for it. Reserved is
/// [`Error::NotPrepared`], anything else [`Error::WrongReservation`].
fn launchable<S: LaunchSteps>(entry: &Entry<S>, folder: &Path) -> Result<(), Error> {
    match entry.state {
        State::Prepared if entry.prepared.as_deref() == Some(folder) => Ok(()),
        State::Reserving | State::Reserved => Err(Error::NotPrepared),
        State::Prepared | State::Preparing | State::Launched | State::Ending | State::Releasing => {
            Err(Error::WrongReservation)
        }
    }
}

impl<S: LaunchSteps> Release for Registry<S> {
    /// Undoes what was done for claim `id` that wasn't launched, newest first: the run's group,
    /// the slot, the session's mark; then frees the matter. A launched one is `after_session`'s.
    fn release(&self, id: u64) {
        let taken = {
            let mut matters = self.lock();
            matters.iter_mut().find_map(|(matter, entry)| {
                let undone = matches!(
                    entry.state,
                    State::Reserving | State::Reserved | State::Preparing | State::Prepared
                );
                (entry.id == id && undone).then(|| {
                    // Busy until the undo is done, so no reservation of the matter overlaps it.
                    entry.state = State::Releasing;
                    let opened = entry.opened.then(|| entry.session.clone());
                    (matter.clone(), entry.run.take(), entry.slot.take(), opened)
                })
            })
        };
        let Some((matter, run, slot, opened)) = taken else {
            return;
        };
        if let Some(run) = run {
            self.steps.end_run(run);
        }
        if let Some(slot) = &slot {
            self.steps.release_slot(slot);
        }
        if let Some(session) = opened {
            self.steps.close_session(&matter, &session);
        }
        self.finish(&matter, id);
    }
}
