//! The markers' checks through a fake, on every OS: F4 refusing a logon until `StopSequence`
//! reaches a stop-awaiting marker's number, a dead outer's marker made one awaiting a stop, the
//! re-mark once that stop completed (its sweep read only then), the final stop read once that
//! sweep completed, a launch's bounded wait for a sweep, and the desktop's start-up requests.

use std::collections::BTreeMap;

use super::super::marker::Outer;
use super::*;

mod failures;
use failures::HealRun;
mod final_stop;
mod install;
mod launch;
mod live;

const POOL_A: &str = "S-1-5-21-1-2-3-1010";
const POOL_B: &str = "S-1-5-21-1-2-3-1011";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// A marker as the fake holds it: its pool user, its state (`None`: unreadable) and its install.
type Held = (Option<Sid>, Option<MarkerState>, Option<String>);

/// Which of the fake's marker effects fail, every time.
#[derive(Default)]
struct Fails {
    settle: bool,
    remove: bool,
}

/// The fake: the markers by id, the record's numbers, and everything done.
struct Fake {
    markers: BTreeMap<String, Held>,
    done: Sequences,
    /// What each pause moves `done.sweep` to, in turn: a sweep run completing.
    sweeps: Vec<u64>,
    stop_next: u64,
    sweep_next: u64,
    pool: Vec<Sid>,
    /// The record's `InstallId`; `Err` when it can't be read.
    install: Result<Option<String>, String>,
    unreadable: bool,
    /// `SweepNext` can't be read.
    sweep_next_fails: bool,
    /// Which of the marker writes and removals fail.
    fails: Fails,
    /// The outer each marker names, by id (none for the rest).
    outers: BTreeMap<String, Outer>,
    /// What opening each pid finds; [`OuterSeen::Gone`] for the rest.
    processes: BTreeMap<u32, OuterSeen>,
    /// What the tasks need, in the readiness check's words.
    task_repair: Option<&'static str>,
    /// What the first reads of the tasks give instead, in order, before `task_repair`.
    queued_task_repairs: Vec<Option<&'static str>>,
    /// How many times the tasks were read.
    task_reads: usize,
    /// Whether the refresh task can be started, and what each run asked for does, in turn; a
    /// run asked for past them changes nothing.
    heal_starts: bool,
    heal_runs: Vec<HealRun>,
    /// The record's `AdminCopySha256`, as a count of the swaps so far.
    swaps: u32,
    log: Vec<String>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            markers: BTreeMap::new(),
            done: Sequences::default(),
            sweeps: Vec::new(),
            stop_next: 5,
            sweep_next: 9,
            pool: vec![sid(POOL_A), sid(POOL_B)],
            install: Ok(None),
            unreadable: false,
            sweep_next_fails: false,
            fails: Fails::default(),
            outers: BTreeMap::new(),
            processes: BTreeMap::new(),
            task_repair: None,
            queued_task_repairs: Vec::new(),
            task_reads: 0,
            heal_starts: false,
            heal_runs: Vec::new(),
            swaps: 0,
            log: Vec::new(),
        }
    }

    fn marker(&mut self, id: &str, user: Option<&str>, state: Option<MarkerState>) {
        self.markers
            .insert(id.to_owned(), (user.map(sid), state, None));
    }

    /// A marker written under the install id `install`.
    fn installed(&mut self, id: &str, state: Option<MarkerState>, install: Option<&str>) {
        let held = (Some(sid(POOL_A)), state, install.map(str::to_owned));
        self.markers.insert(id.to_owned(), held);
    }

    /// The marker's state; `None` when it is gone or unreadable.
    fn state(&self, id: &str) -> Option<MarkerState> {
        self.markers.get(id).and_then(|(_, state, _)| *state)
    }

    fn has(&self, id: &str) -> bool {
        self.markers.contains_key(id)
    }

    fn asked(&self, what: &str) -> usize {
        self.log.iter().filter(|line| *line == what).count()
    }
}

impl Markers for Fake {
    fn sessions(&mut self) -> io::Result<Vec<UncleanSession>> {
        let sessions = self
            .markers
            .iter()
            .map(|(id, (user, state, install))| UncleanSession {
                id: id.clone(),
                pool_user: user.clone(),
                state: *state,
                install: state.and(install.clone()),
                outer: self.outers.get(id).copied(),
            });
        Ok(sessions.collect())
    }
    fn sequences(&mut self) -> Result<Sequences, String> {
        if self.unreadable {
            return Err("RegGetValueW failed (5)".to_owned());
        }
        Ok(self.done)
    }
    fn next_number(&mut self, name: &'static str) -> Result<u64, String> {
        self.log.push(format!("read {name}"));
        if name == SWEEP_NEXT && self.sweep_next_fails {
            return Err("RegGetValueW failed (5)".to_owned());
        }
        Ok(if name == STOP_NEXT {
            self.stop_next
        } else {
            self.sweep_next
        })
    }
    fn pool(&mut self) -> Vec<Sid> {
        self.pool.clone()
    }
    fn install_id(&mut self) -> Result<Option<String>, String> {
        self.install.clone()
    }
    fn settle(
        &mut self,
        id: &str,
        pool_user: Option<&Sid>,
        install: Option<&str>,
        state: MarkerState,
    ) -> io::Result<()> {
        if self.fails.settle {
            return Err(io::Error::from_raw_os_error(32));
        }
        let held = (pool_user.cloned(), Some(state), install.map(str::to_owned));
        self.markers.insert(id.to_owned(), held);
        Ok(())
    }
    fn remove(&mut self, id: &str) -> io::Result<()> {
        if self.fails.remove {
            self.log.push(format!("can't remove {id}"));
            return Err(io::Error::from_raw_os_error(32));
        }
        self.markers.remove(id);
        self.log.push(format!("remove {id}"));
        Ok(())
    }
    fn request_stop(&mut self) -> bool {
        self.log.push("stop".to_owned());
        true
    }
    fn request_sweep(&mut self) -> Result<(), String> {
        self.log.push("sweep".to_owned());
        Ok(())
    }
    fn pause(&mut self, pause: Duration) {
        if pause == HEAL_POLL {
            self.log.push("pause for the heal".to_owned());
            return;
        }
        if pause == TASK_RETRY {
            self.log.push("pause for the tasks".to_owned());
            return;
        }
        assert_eq!(pause, SWEEP_POLL);
        self.log.push("pause".to_owned());
        if !self.sweeps.is_empty() {
            self.done.sweep = self.sweeps.remove(0);
        }
    }
    fn note(&mut self, line: &str) {
        self.log.push(format!("note {line}"));
    }
    fn outer(&mut self, pid: u32) -> OuterSeen {
        self.log.push(format!("open {pid}"));
        self.processes.get(&pid).copied().unwrap_or(OuterSeen::Gone)
    }
    fn heal_tasks(&mut self) -> bool {
        self.log.push("heal".to_owned());
        if self.heal_starts && !self.heal_runs.is_empty() {
            let run = self.heal_runs.remove(0);
            self.swaps += u32::from(run.swaps);
            self.task_repair = run.leaves;
        }
        self.heal_starts
    }
    fn admin_copy_stamp(&mut self) -> Option<String> {
        Some(self.swaps.to_string())
    }
    fn task_repair(&mut self) -> Option<&'static str> {
        self.task_reads += 1;
        if self.queued_task_repairs.is_empty() {
            self.task_repair
        } else {
            self.queued_task_repairs.remove(0)
        }
    }
}

const INSTALL: &str = "0123456789abcdef0123456789abcdef";
const OLD_INSTALL: &str = "ffffffffffffffffffffffffffffffff";

const STOP: MarkerState = MarkerState::AwaitingStop { stop: 3 };
/// What [`STOP`] becomes once its stop completed: the fake's `SweepNext`, read then.
const REMARKED: MarkerState = MarkerState::AwaitingSweepBeforeStop { sweep: 9 };
/// What [`REMARKED`] becomes once its sweep completed: the fake's `StopNext`, read then.
const FINAL: MarkerState = MarkerState::AwaitingFinalStop { stop: 5 };

#[test]
fn f4_refuses_until_stop_sequence_reaches_the_markers_number() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 2, sweep: 2 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert!(
        refused.why.contains("awaits the sandbox's stop"),
        "{refused:?}"
    );
    assert_eq!(fake.asked("stop"), 1, "each refusal asks for the stop");
    assert_eq!(fake.state("a1"), Some(STOP));
    // An absent StopSequence is 0: refused too.
    fake.done = Sequences::default();
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(
        fake.asked("read SweepNext"),
        0,
        "no sweep number while the escapee lives"
    );
    // Once the stop that reserved 3 completed: the marker awaits a sweep whose number is read
    // now, which is requested and waited for; once it completes, the final stop's number is
    // read and that stop requested, and the logon refused until it completes.
    fake.done = Sequences { stop: 3, sweep: 2 };
    fake.sweeps = vec![2, 9];
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert!(refused.why.contains("final stop"), "{refused:?}");
    assert_eq!(fake.state("a1"), Some(FINAL));
    assert_eq!(fake.asked("stop"), 3);
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 2);
    fake.done.stop = 5;
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a1"), "cleared by the final stop");
    assert_eq!(fake.asked("stop"), 3);
}

#[test]
fn f4_waits_for_the_sweep_a_re_mark_requested_and_refuses_without_a_stop_if_it_doesnt_come() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 2 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingSweep, "no stop is awaited");
    assert!(
        refused.why.contains("didn't finish within 15 s"),
        "{refused:?}"
    );
    // Re-marked, its sweep requested once, waited for 15 s, and no stop asked for.
    assert_eq!(fake.state("a1"), Some(REMARKED));
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 150);
    assert_eq!(fake.asked("stop"), 0);
    // A marker awaiting a sweep already, not re-marked now: the sweep is asked for, and waited.
    let mut pending = Fake::new();
    let awaiting = MarkerState::AwaitingSweep { sweep: 7 };
    pending.marker("a2", Some(POOL_A), Some(awaiting));
    pending.done.sweep = 6;
    pending.sweeps = vec![7];
    assert_eq!(logon_check(&mut pending, &sid(POOL_A)), Ok(()));
    assert_eq!(pending.log, ["sweep", "pause", "remove a2"]);
    // One already met: the logon goes at once.
    let mut met = Fake::new();
    met.marker("a3", Some(POOL_A), Some(awaiting));
    met.done.sweep = 7;
    assert_eq!(logon_check(&mut met, &sid(POOL_A)), Ok(()));
    assert_eq!(met.log, ["remove a3"]);
}

#[test]
fn f4_counts_this_pool_users_markers_and_those_naming_none() {
    let mut fake = Fake::new();
    fake.marker("b1", Some(POOL_B), Some(STOP));
    // Another pool user's escapee doesn't hold this one's logon.
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    // One that names no pool user, or can't be read, does, for every pool user.
    fake.marker("x1", None, Some(STOP));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.markers.remove("x1");
    fake.marker("x2", None, None);
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.markers.remove("x2");
    fake.log.clear();
    // Another pool user's marker awaiting a sweep isn't this one's to wait for.
    fake.marker(
        "b2",
        Some(POOL_B),
        Some(MarkerState::AwaitingSweep { sweep: 40 }),
    );
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    // A stop met: the marker is re-marked, and the logon goes once its sweep, then its final
    // stop, completed.
    fake.marker("a3", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 1 };
    fake.sweeps = vec![9];
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(fake.state("a3"), Some(FINAL));
    fake.done.stop = 5;
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a3"));
    // Sequences that can't be read refuse a marker awaiting a stop.
    fake.marker("a4", Some(POOL_A), Some(STOP));
    fake.unreadable = true;
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
}

#[test]
fn f4_makes_a_dead_outers_marker_await_a_stop_read_now() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(MarkerState::Open));
    fake.done = Sequences {
        stop: 99,
        sweep: 99,
    };
    // Refused even though every earlier stop completed: the stop must begin after this read.
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let read = MarkerState::AwaitingStop { stop: 5 };
    assert_eq!(fake.state("a1"), Some(read));
    assert_eq!(fake.log, ["read StopNext", "stop"]);
    // The stop that reserved 5 completed: the marker awaits a sweep, then a final stop read once
    // that sweep completed; the logon goes once that stop completed.
    fake.done = Sequences { stop: 4, sweep: 99 };
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.done.stop = 5;
    fake.stop_next = 6;
    fake.sweep_next = 100;
    fake.sweeps = vec![100];
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let last = MarkerState::AwaitingFinalStop { stop: 6 };
    assert_eq!(fake.state("a1"), Some(last));
    fake.done.stop = 6;
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a1"));
}

#[test]
fn a_marker_naming_a_pool_user_the_record_no_longer_lists_is_stale() {
    let mut fake = Fake::new();
    fake.marker("old", Some("S-1-5-21-9-9-9-1010"), Some(STOP));
    fake.marker("a1", Some(POOL_A), Some(STOP));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert!(!fake.has("old"), "a removal's leftover goes");
    // With no record read, nothing is stale.
    fake.marker("old", Some("S-1-5-21-9-9-9-1010"), Some(STOP));
    fake.pool.clear();
    drop(await_sweeps(&mut fake));
    assert!(fake.has("old"));
}

#[test]
fn the_desktops_start_makes_dead_outers_markers_await_a_stop_and_asks_for_both() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(MarkerState::Open));
    fake.marker("x1", None, None);
    fake.marker(
        "b1",
        Some(POOL_B),
        Some(MarkerState::AwaitingSweep { sweep: 3 }),
    );
    fake.done = Sequences { stop: 0, sweep: 3 };
    // An escapee's whose stop completed: re-marked, and the sweep requested.
    fake.marker(
        "b2",
        Some(POOL_B),
        Some(MarkerState::AwaitingStop { stop: 2 }),
    );
    fake.done = Sequences { stop: 2, sweep: 3 };
    let found = start_up(&mut fake).unwrap();
    let read = MarkerState::AwaitingStop { stop: 5 };
    assert_eq!(fake.state("a1"), Some(read));
    // An unreadable one is written again, naming no pool user, so it counts for all.
    assert_eq!(fake.markers["x1"], (None, Some(read), None));
    assert!(!fake.has("b1"), "a clean one goes");
    assert_eq!(fake.state("b2"), Some(REMARKED));
    let want = StartUp {
        left: 3,
        stop_requested: true,
        sweep_requested: true,
    };
    assert_eq!(found, want);
    // Each number read once, the sweep asked for once; it never waits.
    assert_eq!(fake.asked("read StopNext"), 1);
    assert_eq!(fake.asked("read SweepNext"), 1);
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 0);
    // Nothing awaited: nothing asked for.
    let mut clean = Fake::new();
    clean.marker(
        "b1",
        Some(POOL_B),
        Some(MarkerState::AwaitingSweep { sweep: 0 }),
    );
    assert_eq!(start_up(&mut clean).unwrap(), StartUp::default());
    assert!(!clean.log.contains(&"stop".to_owned()));
}

#[test]
fn f4_replaces_an_unreadable_marker_with_one_awaiting_a_stop_read_now() {
    let mut fake = Fake::new();
    fake.install = Ok(Some(INSTALL.to_owned()));
    fake.marker("x1", None, None);
    // An open one naming no pool user is no live session's either.
    let open = (None, Some(MarkerState::Open), Some(INSTALL.to_owned()));
    fake.markers.insert("x2".to_owned(), open);
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert!(
        refused.why.contains("the stop was requested"),
        "{refused:?}"
    );
    // Both now await the stop that begins after this read, naming no pool user, under the
    // current install: no longer a refusal for good.
    let read = MarkerState::AwaitingStop { stop: 5 };
    let held = (None, Some(read), Some(INSTALL.to_owned()));
    assert_eq!(fake.markers["x1"], held);
    assert_eq!(fake.markers["x2"], held);
    assert_eq!(fake.asked("read StopNext"), 1);
    // Refused until that stop completes, then the logon goes and they await a sweep.
    fake.done.stop = 4;
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.done.stop = 5;
    assert!(
        logon_check(&mut fake, &sid(POOL_A)).is_err(),
        "its sweep is waited for"
    );
    assert_eq!(fake.state("x1"), Some(REMARKED));
    assert_eq!(fake.state("x2"), Some(REMARKED));
    // Then its final stop, read once that sweep completed.
    fake.sweeps = vec![9];
    fake.stop_next = 6;
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let last = MarkerState::AwaitingFinalStop { stop: 6 };
    assert_eq!(fake.state("x1"), Some(last));
    assert_eq!(fake.state("x2"), Some(last));
    fake.done.stop = 6;
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(fake.markers.is_empty(), "{:?}", fake.markers);
}
