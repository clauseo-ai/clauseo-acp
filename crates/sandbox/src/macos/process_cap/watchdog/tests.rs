//! The watchdog's loop on a fake [`System`] and a fake session sweep: what each poll counts,
//! when it trips, what it does to the agent, how it fails closed, and how it counts what the
//! launch's mark finds once the agent's session is empty.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use super::*;
use crate::macos::process_cap::ESCALATE_AT;

/// One fake process.
#[derive(Debug, Clone, Copy)]
struct Fp {
    parent: u32,
    session: u32,
    start: u64,
    zombie: bool,
    /// Survives `SIGKILL` (a process the kill rounds cannot end).
    immortal: bool,
}

/// A fake Mac: its processes, and what the watchdog did to them.
#[derive(Default)]
struct Fake {
    procs: RefCell<BTreeMap<u32, Fp>>,
    /// Every listing fails.
    listing_fails: Cell<bool>,
    /// pids `info` cannot read, though they run.
    unreadable: RefCell<HashSet<u32>>,
    listings: Cell<usize>,
    signals: RefCell<Vec<(u32, c_int)>>,
    groups: RefCell<Vec<(u32, c_int)>>,
}

impl Fake {
    fn add(&self, pid: u32, parent: u32, session: u32, start: u64) {
        self.procs.borrow_mut().insert(
            pid,
            Fp {
                parent,
                session,
                start,
                zombie: false,
                immortal: false,
            },
        );
    }

    fn set(&self, pid: u32, change: impl FnOnce(&mut Fp)) {
        change(self.procs.borrow_mut().get_mut(&pid).unwrap());
    }

    fn running(&self, pid: u32) -> bool {
        self.procs.borrow().get(&pid).is_some_and(|p| !p.zombie)
    }

    fn signals_to(&self, pid: u32) -> Vec<c_int> {
        let signals = self.signals.borrow();
        signals.iter().filter(|s| s.0 == pid).map(|s| s.1).collect()
    }

    /// `n` processes from `first` with parent `parent` in session `session`, started at 50.
    fn many(&self, first: u32, n: u32, parent: u32, session: u32) {
        for pid in first..first + n {
            self.add(pid, parent, session, 50);
        }
    }
}

impl System for Fake {
    fn pids(&self) -> Option<Vec<u32>> {
        self.listings.set(self.listings.get() + 1);
        (!self.listing_fails.get()).then(|| self.procs.borrow().keys().copied().collect())
    }

    fn session_of(&self, pid: u32) -> Option<u32> {
        let procs = self.procs.borrow();
        procs.get(&pid).filter(|p| !p.zombie).map(|p| p.session)
    }

    fn info(&self, pid: u32) -> Option<Proc> {
        if self.unreadable.borrow().contains(&pid) {
            return None;
        }
        let procs = self.procs.borrow();
        procs.get(&pid).filter(|p| !p.zombie).map(|p| Proc {
            pid,
            parent: p.parent,
            session: p.session,
            start: p.start,
            uid: 501,
        })
    }

    fn signal(&self, pid: u32, start: u64, signal: c_int) -> bool {
        if self.info(pid).map(|p| p.start) != Some(start) {
            return false;
        }
        self.signals.borrow_mut().push((pid, signal));
        let immortal = self.procs.borrow()[&pid].immortal;
        if signal == libc::SIGKILL && !immortal {
            self.procs.borrow_mut().remove(&pid);
        }
        true
    }

    fn signal_group(&self, group: u32, signal: c_int) -> bool {
        self.groups.borrow_mut().push((group, signal));
        true
    }

    fn pause(&self, _: Duration) {}
}

/// The agent's pid and start in every test.
const AGENT: u32 = 100;
const START: u64 = 10;

/// The launch's session sweep, faked: the pids its mark finds, whether its scan fails, whether
/// session end ended it, and what trips handed to it.
#[derive(Clone, Default)]
struct Sweep(Arc<Mutex<Swept>>);

#[derive(Default)]
struct Swept {
    marked: HashSet<u32>,
    fails: bool,
    ended: bool,
    handed: Vec<u32>,
}

impl Sweep {
    fn mark(&self, pids: std::ops::Range<u32>) {
        self.0.lock().unwrap().marked.extend(pids);
    }
}

impl Escapes for Sweep {
    fn escaped(&self, rest: &[Proc]) -> Result<Option<Vec<Proc>>, SweepError> {
        let swept = self.0.lock().unwrap();
        if swept.ended {
            return Ok(None);
        }
        if swept.fails {
            return Err(SweepError::Unknown {
                tag: "clauseo-1-0".to_owned(),
            });
        }
        let found = rest.iter().filter(|p| swept.marked.contains(&p.pid));
        Ok(Some(found.copied().collect()))
    }

    fn take_over(&self, left: &[Proc]) -> usize {
        let mut swept = self.0.lock().unwrap();
        swept.handed.extend(left.iter().map(|p| p.pid));
        0
    }
}

/// A session whose notices land in the returned list.
fn session() -> (ProcessCapSession, Arc<Mutex<Vec<ProcessCapNotice>>>) {
    let told = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&told);
    let session = ProcessCapSession::new(move |notice| kept.lock().unwrap().push(notice));
    (session, told)
}

/// A fake with the agent running, and its watch, whose mark finds nothing.
fn agent(cap: &ProcessCapSession) -> (Fake, Watch) {
    let (fake, watch, _) = agent_swept(cap);
    (fake, watch)
}

/// A fake with the agent running, its watch, and the watch's session sweep.
fn agent_swept(cap: &ProcessCapSession) -> (Fake, Watch, Sweep) {
    let fake = Fake::default();
    fake.add(AGENT, 1, AGENT, START);
    fake.add(1, 0, 1, 0);
    let sweep = Sweep::default();
    let watch = Watch::start(&fake, AGENT, cap.clone(), Box::new(sweep.clone()))
        .unwrap()
        .unwrap();
    (fake, watch, sweep)
}

/// The steps up to and including the next sweep's; each but the last only counts.
fn to_the_sweep(watch: &mut Watch, fake: &Fake) -> Step {
    loop {
        let sweep = (watch.polls + 1).is_multiple_of(SWEEP_EVERY);
        let step = watch.step(fake);
        if sweep || step == Step::Done {
            return step;
        }
    }
}

#[test]
fn job_control_orphans_are_counted_by_their_session() {
    // H1: `set -m` gave each job a group of its own, and its parent exited, so launchd adopted
    // it. Neither a group count nor a walk from the agent finds these; the session does.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.many(1_000, 300, 1, AGENT);
    assert_eq!(watch.step(&fake), Step::Watch);
    assert!(
        (1_000..1_300).all(|pid| !fake.running(pid)),
        "the orphans run"
    );
    assert_eq!(fake.signals_to(AGENT), [libc::SIGCONT]);
    assert_eq!(*fake.groups.borrow(), [(AGENT, libc::SIGSTOP)]);
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::TooManyPrograms]);
    assert_eq!(cap.trips(), 1);
}

#[test]
fn at_the_cap_nothing_trips() {
    let (cap, _) = session();
    let (fake, mut watch) = agent(&cap);
    let cap_count = u32::try_from(AGENT_PROCESS_CAP).unwrap();
    fake.many(1_000, cap_count, AGENT, AGENT);
    for _ in 0..SWEEP_EVERY {
        assert_eq!(watch.step(&fake), Step::Watch);
    }
    assert_eq!(cap.trips(), 0);
    assert!(fake.signals.borrow().is_empty());
}

#[test]
fn zombies_never_count_and_one_listing_runs_a_poll() {
    // M2: 400 unreaped children, and the agent forking short-lived ones beside them.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.many(1_000, 400, AGENT, AGENT);
    for pid in 1_000..1_400 {
        fake.set(pid, |p| p.zombie = true);
    }
    fake.many(5_000, 10, AGENT, AGENT);
    let polls = 200;
    for _ in 0..polls {
        assert_eq!(watch.step(&fake), Step::Watch);
    }
    assert_eq!(cap.trips(), 0);
    assert!(told.lock().unwrap().is_empty());
    // One listing a poll, and one more at each sweep's reading.
    let sweeps = polls / usize::try_from(SWEEP_EVERY).unwrap();
    assert_eq!(fake.listings.get(), polls + sweeps);
}

#[test]
fn a_descendant_that_left_the_session_is_found_at_the_sweep() {
    // A child of the agent's command called setsid and bombed; its parent lives on.
    let (cap, _) = session();
    let (fake, mut watch) = agent(&cap);
    fake.add(101, AGENT, AGENT, 20);
    fake.add(500, 101, 500, 30);
    fake.many(1_000, 300, 500, 500);
    for _ in 1..SWEEP_EVERY {
        watch.step(&fake);
    }
    assert_eq!(cap.trips(), 0, "a poll counts the session only");
    watch.step(&fake);
    assert_eq!(cap.trips(), 1);
    assert!(!fake.running(500) && !fake.running(1_000) && fake.running(AGENT));
}

/// One command (pid `child`, the agent's child) with 300 children of its own.
fn bomb(fake: &Fake, root: u32, child: u32) {
    fake.add(child, root, root, 20);
    fake.many(child * 1_000, 300, child, root);
}

#[test]
fn the_third_trip_of_a_session_ends_the_agent() {
    // M1: two trips spare the agent; the third, in a later launch of the same session, ends it.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    bomb(&fake, AGENT, 101);
    watch.step(&fake);
    bomb(&fake, AGENT, 102);
    watch.step(&fake);
    assert_eq!(cap.trips(), 2);
    assert!(fake.running(AGENT));
    assert_eq!(fake.signals_to(AGENT), [libc::SIGCONT, libc::SIGCONT]);
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::TooManyPrograms]);

    let (next, mut second) = agent(&cap);
    bomb(&next, AGENT, 103);
    second.step(&next);
    assert_eq!(cap.trips(), ESCALATE_AT);
    assert!(
        !next.running(AGENT),
        "the agent runs on after the third trip"
    );
    assert_eq!(next.signals_to(AGENT), [libc::SIGKILL]);
    assert_eq!(
        *told.lock().unwrap(),
        [
            ProcessCapNotice::TooManyPrograms,
            ProcessCapNotice::AssistantStopped
        ]
    );
    // Its session is empty now: the watch ends.
    assert_eq!(second.step(&next), Step::Done);
}

#[test]
fn the_session_is_watched_after_the_agent_exits() {
    // L3: `bomb & exit`. The agent is gone; its session's processes are still counted, and the
    // watch ends only once the session is empty.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.procs.borrow_mut().remove(&AGENT);
    fake.add(101, 1, AGENT, 20);
    assert_eq!(watch.step(&fake), Step::Watch);
    fake.many(1_000, 300, 101, AGENT);
    assert_eq!(watch.step(&fake), Step::Watch);
    assert_eq!(cap.trips(), 1);
    assert!(
        fake.signals_to(AGENT).is_empty(),
        "the gone agent was signalled"
    );
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::TooManyPrograms]);
    assert!(!fake.running(101));
    assert_eq!(watch.step(&fake), Step::Done);
}

#[test]
fn a_zombie_agent_is_an_exit_not_a_failure() {
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.add(101, AGENT, AGENT, 20);
    fake.set(AGENT, |p| p.zombie = true);
    assert_eq!(watch.step(&fake), Step::Watch);
    assert!(told.lock().unwrap().is_empty());
}

#[test]
fn what_the_kill_rounds_leave_goes_to_the_session_sweep() {
    // L6: a descendant survives every kill round; the watch hands it over and goes on.
    let (cap, _) = session();
    let (fake, mut watch, sweep) = agent_swept(&cap);
    bomb(&fake, AGENT, 101);
    fake.set(101_010, |p| p.immortal = true);
    let trip = watch.trip(&fake, true, true);
    assert_eq!(
        trip.left.iter().map(|p| p.pid).collect::<Vec<_>>(),
        [101_010]
    );
    assert_eq!(sweep.0.lock().unwrap().handed, [101_010]);
    assert_eq!(watch.step(&fake), Step::Watch);
}

/// Asserts the watch failed closed: the agent's group and the agent got `SIGKILL`, and the
/// lawyer was told.
fn failed_closed(fake: &Fake, told: &Mutex<Vec<ProcessCapNotice>>) {
    assert!(fake.groups.borrow().contains(&(AGENT, libc::SIGKILL)));
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::CapFailed]);
}

#[test]
fn a_failed_listing_ends_the_agent() {
    // M3.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.add(101, AGENT, AGENT, 20);
    fake.listing_fails.set(true);
    assert_eq!(watch.step(&fake), Step::Done);
    assert_eq!(fake.signals_to(AGENT), [libc::SIGKILL]);
    failed_closed(&fake, &told);
}

#[test]
fn an_agent_that_cannot_be_read_is_ended() {
    // M3: the agent runs (its session answers) but `proc_pidinfo` fails on it.
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.add(101, AGENT, AGENT, 20);
    fake.unreadable.borrow_mut().insert(AGENT);
    assert_eq!(watch.step(&fake), Step::Done);
    assert_eq!(fake.signals_to(101), [libc::SIGKILL]);
    failed_closed(&fake, &told);
}

#[test]
fn an_agent_whose_start_changed_is_ended() {
    let (cap, told) = session();
    let (fake, mut watch) = agent(&cap);
    fake.set(AGENT, |p| p.start = START + 1);
    assert_eq!(watch.step(&fake), Step::Done);
    failed_closed(&fake, &told);
}

#[test]
fn an_agent_whose_start_is_missing_is_not_watched_uncounted() {
    // M3: the launch fails (the caller drops the agent's tree) and the lawyer is told.
    let (cap, told) = session();
    let fake = Fake::default();
    fake.add(AGENT, 1, AGENT, START);
    fake.unreadable.borrow_mut().insert(AGENT);
    let refused = Watch::start(&fake, AGENT, cap, Box::new(Sweep::default())).unwrap_err();
    assert_eq!(refused, "the agent's start could not be read");
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::CapFailed]);
}

#[test]
fn an_agent_that_exited_at_once_is_a_normal_end() {
    // D3: gone, or a zombie, with its session empty: no watch, no notice, no error.
    let (cap, told) = session();
    let fake = Fake::default();
    fake.add(1, 0, 1, 0);
    assert!(
        Watch::start(&fake, AGENT, cap.clone(), Box::new(Sweep::default()))
            .unwrap()
            .is_none()
    );
    fake.add(AGENT, 1, AGENT, START);
    fake.set(AGENT, |p| p.zombie = true);
    assert!(
        Watch::start(&fake, AGENT, cap, Box::new(Sweep::default()))
            .unwrap()
            .is_none()
    );
    assert!(told.lock().unwrap().is_empty());
}

#[test]
fn an_agent_that_exited_at_once_leaving_a_bomb_is_watched_by_session() {
    let (cap, told) = session();
    let fake = Fake::default();
    fake.add(1, 0, 1, 0);
    fake.many(1_000, 300, 1, AGENT);
    let mut watch = Watch::start(&fake, AGENT, cap.clone(), Box::new(Sweep::default()))
        .unwrap()
        .unwrap();
    assert_eq!(watch.step(&fake), Step::Watch);
    assert_eq!(cap.trips(), 1);
    assert!((1_000..1_300).all(|pid| !fake.running(pid)));
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::TooManyPrograms]);
    assert_eq!(watch.step(&fake), Step::Done);
}

#[test]
fn what_the_mark_finds_is_counted_after_the_session_empties() {
    // M1: `setsid <bomb> &`, then the agent is killed: its session is empty, but the launch's
    // mark still finds the bomb, so the watch goes on counting it until it finds nothing.
    let (cap, told) = session();
    let (fake, mut watch, sweep) = agent_swept(&cap);
    fake.procs.borrow_mut().remove(&AGENT);
    fake.many(2_000, 10, 1, 2_000);
    sweep.mark(2_000..2_010);
    assert_eq!(
        watch.step(&fake),
        Step::Watch,
        "the escapees went uncounted"
    );
    assert_eq!(cap.trips(), 0);
    // The bomb grows; between sweeps only the session is listed, at the sweep the mark counts.
    fake.many(3_000, 300, 2_000, 2_000);
    sweep.mark(3_000..3_300);
    assert_eq!(to_the_sweep(&mut watch, &fake), Step::Watch);
    assert_eq!(cap.trips(), 1);
    assert!(
        (2_000..2_010)
            .chain(3_000..3_300)
            .all(|pid| !fake.running(pid))
    );
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::TooManyPrograms]);
    // The session is gone: neither its group nor the agent's pid is ever signalled.
    assert!(fake.groups.borrow().is_empty());
    assert!(
        fake.signals_to(AGENT).is_empty(),
        "the gone agent was signalled"
    );
    // Nothing left: the next sweep ends the watch.
    assert_eq!(to_the_sweep(&mut watch, &fake), Step::Done);
}

#[test]
fn session_end_ends_the_watch_of_what_the_mark_found() {
    let (cap, told) = session();
    let (fake, mut watch, sweep) = agent_swept(&cap);
    fake.procs.borrow_mut().remove(&AGENT);
    fake.many(2_000, 10, 1, 2_000);
    sweep.mark(2_000..2_010);
    assert_eq!(watch.step(&fake), Step::Watch);
    sweep.0.lock().unwrap().ended = true;
    assert_eq!(to_the_sweep(&mut watch, &fake), Step::Done);
    assert!(told.lock().unwrap().is_empty());
}

#[test]
fn a_failed_session_sweep_ends_the_agent() {
    // M3: the mark's scan fails (an unvouched scan, drift, a missing mark): the cap cannot
    // count what left the session, so it fails closed as for a failed listing.
    let (cap, told) = session();
    let (fake, mut watch, sweep) = agent_swept(&cap);
    fake.add(101, AGENT, AGENT, 20);
    sweep.0.lock().unwrap().fails = true;
    assert_eq!(to_the_sweep(&mut watch, &fake), Step::Done);
    assert_eq!(fake.signals_to(AGENT), [libc::SIGKILL]);
    assert_eq!(fake.signals_to(101), [libc::SIGKILL]);
    failed_closed(&fake, &told);
}

#[test]
fn a_failed_session_sweep_after_the_session_emptied_is_told() {
    let (cap, told) = session();
    let (fake, mut watch, sweep) = agent_swept(&cap);
    fake.procs.borrow_mut().remove(&AGENT);
    sweep.0.lock().unwrap().fails = true;
    assert_eq!(watch.step(&fake), Step::Done);
    assert_eq!(*told.lock().unwrap(), [ProcessCapNotice::CapFailed]);
    // No group of the agent's pid exists any longer: none is signalled.
    assert!(fake.groups.borrow().is_empty());
}
