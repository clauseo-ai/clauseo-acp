//! [`Counts`], the handle the host and desktop hold: a cheap clone over two mutexes.
//!
//! The state's mutex guards memory only, and is never held across a file change: `bump` takes it
//! alone and never waits on the disk. The disk mutex orders the file changes. Whatever changes
//! files takes the disk mutex first, then the state's for as long as it changes memory and takes
//! the changes the state queued, and makes them with only the disk mutex held. So an off, which
//! waits for the disk mutex, deletes the folder after any write already under way, and a write
//! queued before it is dropped (`State::turn_off`): nothing written before an off outlives it.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::about::About;
use crate::schema::{Day, DayCells, Event, InstallId, Time};
use crate::sender::{Outcome, Post, PostId};
use crate::state::State;
use crate::store::{Op, Store};

/// Whether counts are kept, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Off, by the lawyer or by policy: nothing is counted, saved or sent, and the counts folder
    /// is deleted.
    Off,
    /// On, before the first act: counted in memory only; nothing is saved or sent.
    Memory,
    /// On, after the first act: counted, saved under `<data_dir>/counts/` and sent.
    Saving,
}

/// One install's counts. Cloning it is cheap; every clone is the same counts.
#[derive(Debug, Clone)]
pub struct Counts {
    state: Arc<Mutex<State>>,
    disk: Arc<Mutex<Disk>>,
}

/// What makes the file changes; its mutex is the disk mutex.
struct Disk {
    store: Store,
    /// Runs before each change, so a test can hold a change under way.
    #[cfg(test)]
    before_change: Option<Box<dyn FnMut() + Send>>,
}

impl Disk {
    fn apply(&mut self, op: &Op) -> io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = self.before_change.as_mut() {
            hook();
        }
        self.store.apply(op)
    }
}

impl fmt::Debug for Disk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disk")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

/// A lock, through a panic while it was held: what is left is plain data, and counts carry on
/// with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Counts {
    /// Opens the counts of `data_dir`. `effective_on` is `privacy.counts` after policy;
    /// `consented` is whether `<data_dir>/first_act_done` exists; `seed` seeds the send delays
    /// ([`crate::os_seed`] in the app). Off deletes `<data_dir>/counts/`. On before the first act
    /// deletes it too and counts in memory. On after it reads the folder back (a file that does
    /// not read as its type is removed) and makes the id if it is missing.
    ///
    /// Never fails: a file error comes back from the first [`Counts::tick`], and a failed delete
    /// is tried again on each tick.
    pub fn open(
        data_dir: &Path,
        about: About,
        effective_on: bool,
        consented: bool,
        seed: u64,
    ) -> Counts {
        let (mut state, result) = State::open(data_dir, about, effective_on, consented, seed);
        state.open_error = result.err();
        let disk = Disk {
            store: state.store.clone(),
            #[cfg(test)]
            before_change: None,
        };
        Counts {
            state: Arc::new(Mutex::new(state)),
            disk: Arc::new(Mutex::new(disk)),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// The disk mutex. Taken before the state's, never while holding it.
    fn disk(&self) -> MutexGuard<'_, Disk> {
        lock(&self.disk)
    }

    /// Makes the file changes the state queued, in order, holding only the disk mutex. A failed
    /// save of the open day or a tally is made again at the next save, and a failed delete of
    /// the folder on the next tick. Returns the first error.
    fn apply(&self, disk: &mut Disk) -> io::Result<()> {
        let ops = std::mem::take(&mut self.state().pending);
        let mut first = Ok(());
        let mut resave = false;
        let mut deleted = None;
        for op in &ops {
            let result = disk.apply(op);
            match op {
                Op::DeleteAll => deleted = Some(result.is_ok()),
                Op::Today(_) | Op::Tally(_) => resave |= result.is_err(),
                _ => {}
            }
            if let Err(err) = result
                && first.is_ok()
            {
                first = Err(err);
            }
        }
        if resave || deleted.is_some() {
            let mut state = self.state();
            state.dirty |= resave;
            if let Some(deleted) = deleted {
                state.delete_pending = !deleted;
            }
        }
        first
    }

    /// Deletes the counts folder now, ahead of anything queued.
    fn delete_now(&self, disk: &mut Disk) -> io::Result<()> {
        let result = disk.apply(&Op::DeleteAll);
        self.state().delete_pending = result.is_err();
        result
    }

    /// Counts `event` once: one lock and one increment. Nothing while off.
    pub fn bump(&self, event: Event) {
        self.state().bump(event);
    }

    /// The window came to the foreground, or left it, at `now`. The foreground time makes the
    /// day's `time_open` cell.
    pub fn set_foreground(&self, foreground: bool, now: Time) {
        let mut state = self.state();
        state.advance(now);
        state.foreground = foreground.then_some(now);
    }

    /// Moves time to `now` (desktop calls it every 60 s and at quit): adds up the foreground
    /// time, closes the day at UTC midnight and the week at its end, drops what is too old, and
    /// saves what changed. A delete that failed is tried again first.
    pub fn tick(&self, now: Time) -> io::Result<()> {
        let mut disk = self.disk();
        let mut state = self.state();
        if let Some(err) = state.open_error.take() {
            return Err(err);
        }
        if state.delete_pending {
            drop(state);
            self.delete_now(&mut disk)?;
            state = self.state();
        }
        state.advance(now);
        state.save();
        drop(state);
        self.apply(&mut disk)
    }

    /// The POST due at `now`: at most one, and none while one is in flight or before the first
    /// act. Each answer goes back through [`Counts::sent`]; [`crate::send_due`] does both.
    pub fn due(&self, now: Time) -> Vec<Post> {
        let mut state = self.state();
        state.advance(now);
        crate::sender::due(&mut state, now).into_iter().collect()
    }

    /// The answer to POST `id`. A 2xx or `400` takes its days or tally out of the queue; any other
    /// status, or none, is a failed try.
    pub fn sent(&self, id: PostId, outcome: Outcome) {
        let mut disk = self.disk();
        let mut state = self.state();
        crate::sender::sent(&mut state, id, outcome);
        state.save();
        drop(state);
        // A failed write is tried again on the next tick.
        drop(self.apply(&mut disk));
    }

    /// The lawyer's switch, or policy, after the first act or at a settings reload. Off cancels
    /// a POST in flight, forgets everything and deletes `<data_dir>/counts/`; on makes a new id.
    pub fn set_enabled(&self, on: bool) -> io::Result<()> {
        let mut disk = self.disk();
        self.state().switches += 1;
        if on {
            let state = self.state();
            if state.mode != crate::Mode::Off {
                return Ok(());
            }
            let delete_first = state.delete_pending;
            drop(state);
            // While the old folder cannot go, counts stay off and the error comes back.
            if delete_first {
                self.delete_now(&mut disk)?;
            }
            self.state().turn_on();
        } else {
            self.state().turn_off();
        }
        self.apply(&mut disk)
    }

    /// The lawyer's first act (`FirstAct { counts }`, or the first change of the switch): writes
    /// `<data_dir>/first_act_done` and, with counts on, starts saving what memory holds. With
    /// `counts` false, counts turn off. Once the marker exists, it changes nothing.
    pub fn first_act(&self, counts: bool) -> io::Result<()> {
        let mut disk = self.disk();
        self.state().switches += 1;
        if self.state().consented {
            return Ok(());
        }
        disk.apply(&Op::Marker)?;
        self.state().first_act(counts);
        self.apply(&mut disk)
    }

    /// New envelope values, from the next day or tally that closes.
    pub fn set_about(&self, about: About) {
        self.state().about = about;
    }

    /// Whether the lawyer's first act has happened: `<data_dir>/first_act_done` existed at
    /// [`Counts::open`] or [`Counts::first_act`] wrote it. The host shows it as the settings'
    /// `counts_started`.
    pub fn consented(&self) -> bool {
        self.state().consented
    }

    /// How many times the switch has been set (`set_enabled` or `first_act`, by anyone). A
    /// reader of the switch's setting takes it before the read: when it has moved by the time the
    /// value is used, the value may be older than the switch, and must not turn counts off.
    pub fn switch_generation(&self) -> u64 {
        self.state().switches
    }

    /// Whether counts are off, in memory or saving.
    pub fn mode(&self) -> Mode {
        self.state().mode
    }

    /// The open day's cells so far (`time_open` comes at its close).
    pub fn today(&self) -> DayCells {
        self.state().today.counter.to_cells()
    }

    /// The closed days waiting to be sent, oldest first.
    pub fn queued_days(&self) -> Vec<Day> {
        self.state().queue.keys().copied().collect()
    }

    /// The install id: saved after the first act, in memory before it, none while off.
    pub fn install_id(&self) -> Option<InstallId> {
        self.state().install
    }
}

#[cfg(test)]
mod tests;
