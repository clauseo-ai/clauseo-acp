//! What one install holds in memory: the open day, the queue, the tallies and the sender, and
//! the mode that says whether any of it is saved. [`crate::Counts`] wraps it in a mutex.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crate::Mode;
use crate::about::About;
use crate::random::Rng;
use crate::schema::{
    Crash, DAY_SECS, Day, DayCounter, DayReport, Event, InstallId, MAX_DAY_AGE,
    MAX_TALLY_AGE_WEEKS, OnboardingStep, SchemaVersion, TallyCounter, TallyId, TallyReport, Time,
    Week,
};
use crate::sender::Sender;
use crate::store::{Store, TallyFile, TodayFile};

/// Foreground time between two calls further apart than this is not counted: the machine slept
/// or the app hung, and the time is not use.
const MAX_FOREGROUND_GAP_SECS: u64 = 300;

/// The open day.
#[derive(Debug, Clone, Default)]
pub(crate) struct Today {
    pub(crate) day: Option<Day>,
    pub(crate) counter: DayCounter,
    pub(crate) foreground_secs: u64,
    pub(crate) steps: BTreeSet<OnboardingStep>,
    pub(crate) posted: Option<Day>,
}

/// A week's tally.
#[derive(Debug, Clone)]
pub(crate) enum Tally {
    Open(TallyCounter),
    Closed { report: TallyReport, send_at: Time },
}

/// Everything one install holds.
#[derive(Debug)]
pub(crate) struct State {
    pub(crate) store: Store,
    pub(crate) about: About,
    pub(crate) mode: Mode,
    pub(crate) consented: bool,
    /// The counts folder must go, and its last delete failed: tried again on each tick.
    pub(crate) delete_pending: bool,
    pub(crate) rng: Rng,
    pub(crate) install: Option<InstallId>,
    pub(crate) today: Today,
    /// Crashes counted before the first tick said which week it is.
    pub(crate) unplaced: Vec<Crash>,
    /// When the app is in the foreground: the moment its time was last added up.
    pub(crate) foreground: Option<Time>,
    pub(crate) queue: BTreeMap<Day, DayReport>,
    pub(crate) tallies: BTreeMap<Week, Tally>,
    pub(crate) sender: Sender,
    /// Something changed that `today.json` or a tally file does not hold yet.
    pub(crate) dirty: bool,
    /// The file error `open` met, for the first tick to return.
    pub(crate) open_error: Option<io::Error>,
}

impl State {
    /// The state for `data_dir`. Off (effective off) deletes the counts folder; on before the
    /// first act deletes it too and keeps everything in memory; on after it reads the folder.
    pub(crate) fn open(
        data_dir: &Path,
        about: About,
        on: bool,
        consented: bool,
        seed: u64,
    ) -> (State, io::Result<()>) {
        let mut state = State {
            store: Store::new(data_dir),
            about,
            mode: Mode::Off,
            consented,
            delete_pending: false,
            rng: Rng::new(seed),
            install: None,
            today: Today::default(),
            unplaced: Vec::new(),
            foreground: None,
            queue: BTreeMap::new(),
            tallies: BTreeMap::new(),
            sender: Sender::default(),
            dirty: false,
            open_error: None,
        };
        let result = match (on, consented) {
            (false, _) => state.delete_folder(),
            (true, false) => {
                state.mode = Mode::Memory;
                state.install = Some(InstallId::random());
                state.delete_folder()
            }
            (true, true) => state.load(),
        };
        (state, result)
    }

    /// Reads the counts folder into memory and starts saving. The id is made when missing.
    fn load(&mut self) -> io::Result<()> {
        self.mode = Mode::Saving;
        let install = if let Some(id) = self.store.read_install() {
            id
        } else {
            let id = InstallId::random();
            self.store.write_install(id)?;
            id
        };
        self.install = Some(install);
        let today = self.store.read_today()?;
        let mut counter = DayCounter::default();
        counter.merge(&today.cells);
        self.today = Today {
            day: today.day,
            counter,
            foreground_secs: today.foreground_secs,
            steps: today.steps,
            posted: today.posted,
        };
        self.queue = self.store.read_queue(install)?;
        self.tallies = self
            .store
            .read_tallies(install)?
            .into_iter()
            .map(|(week, file)| (week, Tally::from(file)))
            .collect();
        Ok(())
    }

    /// Writes everything held in memory: at the first act, and on again after the first act.
    fn save_all(&mut self) -> io::Result<()> {
        if let Some(id) = self.install {
            self.store.write_install(id)?;
        }
        for report in self.queue.values() {
            self.store.write_queued(report)?;
        }
        self.dirty = true;
        self.save()
    }

    /// Writes `today.json` and every tally file when something changed and the mode saves.
    pub(crate) fn save(&mut self) -> io::Result<()> {
        if self.mode != Mode::Saving || !self.dirty {
            return Ok(());
        }
        self.store.write_today(&TodayFile {
            day: self.today.day,
            cells: self.today.counter.to_cells(),
            foreground_secs: self.today.foreground_secs,
            steps: self.today.steps.clone(),
            posted: self.today.posted,
        })?;
        if let Some(install) = self.install {
            for (&week, tally) in &self.tallies {
                self.store.write_tally(&tally.to_file(week, install))?;
            }
        }
        self.dirty = false;
        Ok(())
    }

    /// Deletes the counts folder, or marks it to be tried again on the next tick.
    pub(crate) fn delete_folder(&mut self) -> io::Result<()> {
        let result = self.store.delete_all();
        self.delete_pending = result.is_err();
        result
    }

    /// Counts `event` once, in memory. Nothing is counted while off.
    pub(crate) fn bump(&mut self, event: Event) {
        if self.mode == Mode::Off {
            return;
        }
        match event {
            Event::Onboarding(step) if !self.today.steps.insert(step.step) => return,
            Event::Crash(crash) => match self.today.day {
                Some(day) => {
                    self.with_tally(day.week(), |tally| tally.crash(crash));
                }
                None => self.unplaced.push(crash),
            },
            _ => {}
        }
        self.today.counter.bump(event);
        self.dirty = true;
    }

    /// Runs `f` on the tally counting `week`, opened when there is none. A week already closed
    /// (the clock went back) is not counted into.
    fn with_tally<R>(&mut self, week: Week, f: impl FnOnce(&mut TallyCounter) -> R) -> Option<R> {
        let tally = self
            .tallies
            .entry(week)
            .or_insert_with(|| Tally::Open(TallyCounter::default()));
        match tally {
            Tally::Open(counter) => Some(f(counter)),
            Tally::Closed { .. } => None,
        }
    }

    /// Moves time to `now`: adds up the foreground time, closes the day at each UTC midnight,
    /// closes each finished week, marks this week active and drops what is too old to send.
    pub(crate) fn advance(&mut self, now: Time) {
        if self.mode == Mode::Off {
            return;
        }
        let Some(day) = now.day() else {
            return;
        };
        let open = *self.today.day.get_or_insert(day);
        let mut carry = 0;
        if let Some(since) = self.foreground {
            self.foreground = Some(now);
            let gap = now.since(since);
            if gap <= MAX_FOREGROUND_GAP_SECS {
                let day_end = open.start().plus(DAY_SECS);
                let before = day_end.min(now).since(since);
                self.today.foreground_secs = self.today.foreground_secs.saturating_add(before);
                carry = gap - before;
            }
        }
        if open < day {
            self.close_day();
            self.today.day = Some(day);
            self.sender.new_day(day, &mut self.rng);
        }
        self.today.foreground_secs = self.today.foreground_secs.saturating_add(carry);
        if carry > 0 || self.foreground.is_some() {
            self.dirty = true;
        }
        for crash in std::mem::take(&mut self.unplaced) {
            self.with_tally(day.week(), |tally| tally.crash(crash));
        }
        if self.with_tally(day.week(), TallyCounter::mark_active) == Some(true) {
            self.dirty = true;
        }
        self.close_weeks(day.week());
        self.drop_old(day);
        self.sender.start(now, &mut self.rng);
    }

    /// Turns the open day into a queued report, unless it counted nothing.
    fn close_day(&mut self) {
        let today = std::mem::take(&mut self.today);
        self.today.steps = today.steps;
        self.today.posted = today.posted;
        self.dirty = true;
        let (Some(day), Some(install)) = (today.day, self.install) else {
            return;
        };
        let mut counter = today.counter;
        counter.set_time_open(today.foreground_secs);
        if counter.is_empty() {
            return;
        }
        let about = self.about;
        let report = DayReport {
            schema: SchemaVersion::V1,
            install,
            day,
            version: about.version,
            channel: about.channel,
            os: about.platform.os(),
            os_major: about.platform.os_major(),
            arch: about.arch,
            language: about.language,
            cells: counter.to_cells(),
        };
        if self.mode == Mode::Saving {
            // A failed write keeps the report in memory, and the sender sends it from there.
            drop(self.store.write_queued(&report));
        }
        self.queue.insert(day, report);
        while self.queue.len() > usize::try_from(MAX_DAY_AGE).unwrap_or(usize::MAX) {
            if let Some((oldest, _)) = self.queue.pop_first() {
                self.forget_day(oldest);
            }
        }
    }

    /// Closes every open tally of a week before `this_week`, with its own id and a random
    /// moment in the week after it to be sent.
    fn close_weeks(&mut self, this_week: Week) {
        let about = self.about;
        for (&week, tally) in &mut self.tallies {
            let Tally::Open(counter) = tally else {
                continue;
            };
            if week >= this_week {
                continue;
            }
            let report = TallyReport {
                schema: SchemaVersion::V1,
                tally: TallyId::random(),
                week,
                version: about.version,
                channel: about.channel,
                os: about.platform.os(),
                os_major: about.platform.os_major(),
                arch: about.arch,
                install_type: about.install_type,
                managed: about.managed,
                cells: counter.to_cells(),
            };
            let send_at = week
                .next()
                .monday()
                .start()
                .plus(self.rng.below(7 * DAY_SECS));
            *tally = Tally::Closed { report, send_at };
            self.dirty = true;
        }
    }

    /// Drops queued days older than 14 days and tallies older than 4 weeks.
    fn drop_old(&mut self, today: Day) {
        let old: Vec<Day> = self
            .queue
            .keys()
            .copied()
            .filter(|&day| today.days_since(day) > MAX_DAY_AGE)
            .collect();
        for day in old {
            self.queue.remove(&day);
            self.forget_day(day);
        }
        let this_week = today.week();
        let old: Vec<Week> = self
            .tallies
            .keys()
            .copied()
            .filter(|&week| this_week.weeks_since(week) > MAX_TALLY_AGE_WEEKS)
            .collect();
        for week in old {
            self.forget_tally(week);
        }
    }

    /// Removes a queued day's file. A failed remove leaves a file the next `open` reads back,
    /// and the age rule drops it again.
    pub(crate) fn forget_day(&mut self, day: Day) {
        if self.mode == Mode::Saving {
            drop(self.store.remove_queued(day));
        }
    }

    /// Drops a tally and removes its file.
    pub(crate) fn forget_tally(&mut self, week: Week) {
        self.tallies.remove(&week);
        if self.mode == Mode::Saving {
            drop(self.store.remove_tally(week));
        }
    }

    /// Turns counts off: cancels a POST in flight (it is never retried), forgets everything in
    /// memory, and deletes the counts folder.
    pub(crate) fn turn_off(&mut self) -> io::Result<()> {
        self.mode = Mode::Off;
        self.sender.reset();
        self.install = None;
        self.today = Today::default();
        self.unplaced.clear();
        self.foreground = None;
        self.queue.clear();
        self.tallies.clear();
        self.dirty = false;
        self.delete_folder()
    }

    /// Turns counts on with a new id: saved after the first act, in memory before it. A counts
    /// folder whose delete failed is deleted first; while that fails, counts stay off and the
    /// error comes back, so nothing from before off can sit beside the new id.
    pub(crate) fn turn_on(&mut self) -> io::Result<()> {
        if self.mode != Mode::Off {
            return Ok(());
        }
        if self.delete_pending {
            self.delete_folder()?;
        }
        self.install = Some(InstallId::random());
        if self.consented {
            self.mode = Mode::Saving;
            self.save_all()
        } else {
            self.mode = Mode::Memory;
            Ok(())
        }
    }

    /// The first act: writes the marker; with counts on, starts saving what memory holds.
    pub(crate) fn first_act(&mut self, counts: bool) -> io::Result<()> {
        if self.consented {
            return Ok(());
        }
        self.store.write_marker()?;
        self.consented = true;
        if !counts {
            return self.turn_off();
        }
        if self.mode == Mode::Memory {
            self.mode = Mode::Saving;
            return self.save_all();
        }
        Ok(())
    }
}

impl Tally {
    fn to_file(&self, week: Week, install: InstallId) -> TallyFile {
        match self {
            Tally::Open(counter) => TallyFile::Open {
                install,
                week,
                cells: counter.to_cells(),
            },
            Tally::Closed { report, send_at } => TallyFile::Closed {
                install,
                report: report.clone(),
                send_at: *send_at,
            },
        }
    }
}

impl From<TallyFile> for Tally {
    fn from(file: TallyFile) -> Tally {
        match file {
            TallyFile::Open { cells, .. } => {
                let mut counter = TallyCounter::default();
                counter.merge(&cells);
                Tally::Open(counter)
            }
            TallyFile::Closed {
                report, send_at, ..
            } => Tally::Closed { report, send_at },
        }
    }
}
