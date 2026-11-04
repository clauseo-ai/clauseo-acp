//! [`Counts`], the handle the host and desktop hold: a cheap clone over one mutex.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::about::About;
use crate::schema::{Day, DayCells, Event, InstallId, Time};
use crate::sender::{Outcome, Post, PostId};
use crate::state::State;

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
pub struct Counts(Arc<Mutex<State>>);

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
        Counts(Arc::new(Mutex::new(state)))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves plain data behind; counts carry on with it.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
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
        let mut state = self.state();
        if let Some(err) = state.open_error.take() {
            return Err(err);
        }
        if state.delete_pending {
            state.delete_folder()?;
        }
        state.advance(now);
        state.save()
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
        let mut state = self.state();
        crate::sender::sent(&mut state, id, outcome);
        // A failed write is tried again on the next tick.
        drop(state.save());
    }

    /// The lawyer's switch, or policy, after the first act or at a settings reload. Off cancels
    /// a POST in flight, forgets everything and deletes `<data_dir>/counts/`; on makes a new id.
    pub fn set_enabled(&self, on: bool) -> io::Result<()> {
        let mut state = self.state();
        if on {
            state.turn_on()
        } else {
            state.turn_off()
        }
    }

    /// The lawyer's first act (`FirstAct { counts }`, or the first change of the switch): writes
    /// `<data_dir>/first_act_done` and, with counts on, starts saving what memory holds. With
    /// `counts` false, counts turn off. Once the marker exists, it changes nothing.
    pub fn first_act(&self, counts: bool) -> io::Result<()> {
        self.state().first_act(counts)
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
