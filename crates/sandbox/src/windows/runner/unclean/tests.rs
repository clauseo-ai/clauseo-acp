//! The markers' checks through a fake, on every OS: F4 refusing a logon until `StopSequence`
//! reaches a stop-awaiting marker's number, a dead outer's marker made one awaiting a stop, the
//! re-mark once that stop completed (its sweep read only then), a launch's bounded wait for a
//! sweep, and the desktop's start-up requests.

use std::collections::BTreeMap;

use super::*;

const POOL_A: &str = "S-1-5-21-1-2-3-1010";
const POOL_B: &str = "S-1-5-21-1-2-3-1011";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// A marker as the fake holds it: its pool user, its state (`None`: unreadable) and its install.
type Held = (Option<Sid>, Option<MarkerState>, Option<String>);

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
        let held = (pool_user.cloned(), Some(state), install.map(str::to_owned));
        self.markers.insert(id.to_owned(), held);
        Ok(())
    }
    fn remove(&mut self, id: &str) -> io::Result<()> {
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
        assert_eq!(pause, SWEEP_POLL);
        self.log.push("pause".to_owned());
        if !self.sweeps.is_empty() {
            self.done.sweep = self.sweeps.remove(0);
        }
    }
}

const STOP: MarkerState = MarkerState::AwaitingStop { stop: 3 };
/// What [`STOP`] becomes once its stop completed: the fake's `SweepNext`, read then.
const REMARKED: MarkerState = MarkerState::AwaitingSweep { sweep: 9 };

#[test]
fn f4_refuses_until_stop_sequence_reaches_the_markers_number() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 2, sweep: 2 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert!(refused.contains("awaits the sandbox's stop"), "{refused}");
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
    // now, which is requested and waited for; once it completes, the logon goes.
    fake.done = Sequences { stop: 3, sweep: 2 };
    fake.sweeps = vec![2, 9];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a1"), "cleared by the sweep F4 waited for");
    assert_eq!(fake.asked("stop"), 2);
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 2);
}

#[test]
fn f4_waits_for_the_sweep_a_re_mark_requested_and_refuses_without_a_stop_if_it_doesnt_come() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 2 };
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert!(refused.contains("didn't finish within 15 s"), "{refused}");
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
    // A stop met: the marker is re-marked, and the logon goes once its sweep completed.
    fake.marker("a3", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 1 };
    fake.sweeps = vec![9];
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
    // The stop that reserved 5 completed: the next logon goes, the marker awaiting a sweep.
    fake.done = Sequences { stop: 4, sweep: 99 };
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.done.stop = 5;
    fake.sweep_next = 100;
    fake.sweeps = vec![100];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a1"));
}

#[test]
fn a_launch_clears_a_marker_whose_sweep_completed_without_waiting() {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 4 }),
    );
    fake.marker("a2", Some(POOL_A), Some(MarkerState::Open));
    fake.done = Sequences { stop: 0, sweep: 4 };
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert!(!fake.has("a1"));
    // An open marker may be a live session's: left alone, and nothing asked for.
    assert_eq!(fake.state("a2"), Some(MarkerState::Open));
    assert_eq!(fake.log, ["remove a1"]);
}

#[test]
fn a_launch_asks_for_a_sweep_and_waits_for_it_bounded() {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    fake.marker(
        "b1",
        Some(POOL_B),
        Some(MarkerState::AwaitingSweep { sweep: 7 }),
    );
    fake.done = Sequences { stop: 0, sweep: 5 };
    // A run completes 6, then 7, on the second and third looks.
    fake.sweeps = vec![5, 6, 7];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 3);
    assert!(fake.markers.is_empty(), "{:?}", fake.markers);
    // A sweep that never completes: 150 looks, 15 s, then the refusal; the marker stays.
    let mut stuck = Fake::new();
    stuck.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    let refused = await_sweeps(&mut stuck).unwrap_err();
    assert!(refused.contains("didn't finish within 15 s"), "{refused}");
    assert_eq!(stuck.asked("pause"), 150);
    assert_eq!(SWEEP_WAIT, Duration::from_secs(15));
    assert!(
        SWEEP_WAIT < Duration::from_secs(180),
        "inside the host's launch bound"
    );
    assert!(stuck.has("a1"));
}

#[test]
fn a_stop_awaiting_marker_holds_a_launch_only_once_its_stop_completed() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 0, sweep: 2 };
    // Its stop is F4's: the launch goes, the marker stays, and no sweep number is read.
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.state("a1"), Some(STOP));
    assert!(fake.log.is_empty(), "{:?}", fake.log);
    // Sequences that can't be read refuse.
    fake.unreadable = true;
    assert!(await_sweeps(&mut fake).is_err());
}

#[test]
fn a_sweep_before_the_stop_doesnt_clear_an_escapees_marker_and_one_after_it_does() {
    let mut fake = Fake::new();
    let escapee = MarkerState::AwaitingStop { stop: 5 };
    fake.marker("a1", Some(POOL_A), Some(escapee));
    // Sweeps complete while the escapee lives: it could register a task after any of them.
    fake.done = Sequences { stop: 4, sweep: 50 };
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(
        fake.state("a1"),
        Some(escapee),
        "not cleared by an earlier sweep"
    );
    assert_eq!(fake.asked("read SweepNext"), 0);
    // The stop completes, the escapee dead. `SweepNext` is read now: 51, past every sweep so far.
    fake.done.stop = 5;
    fake.sweep_next = 51;
    fake.log.clear();
    // The launch re-marks it, requests the sweep once, and waits for it.
    fake.sweeps = vec![50, 51];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert!(!fake.has("a1"), "cleared by the sweep after the stop");
    let first = ["read SweepNext", "sweep", "pause", "pause", "remove a1"];
    assert_eq!(fake.log, first);
}

#[test]
fn a_marker_whose_stop_completed_but_cant_be_re_marked_refuses_a_launch() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 99 };
    fake.sweep_next_fails = true;
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(refused.contains("completed stop"), "{refused}");
    assert_eq!(fake.state("a1"), Some(STOP));
    // F4 lets the logon go: the stop it would ask for has completed.
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert_eq!(fake.asked("stop"), 0);
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
    assert!(refused.contains("the stop was requested"), "{refused}");
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
    fake.sweeps = vec![9];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(fake.markers.is_empty(), "{:?}", fake.markers);
}

const INSTALL: &str = "0123456789abcdef0123456789abcdef";
const OLD_INSTALL: &str = "ffffffffffffffffffffffffffffffff";

#[test]
fn a_marker_of_another_install_is_stale_and_removed() {
    let mut fake = Fake::new();
    fake.install = Ok(Some(INSTALL.to_owned()));
    // A removed install's escapee marker, a reinstall's counters starting again: it would
    // refuse every logon until the new StopSequence reached its number.
    let stuck = MarkerState::AwaitingStop { stop: 900 };
    fake.installed("old", Some(stuck), Some(OLD_INSTALL));
    // One from before the ids, after the reinstall wrote one.
    fake.installed("none", Some(stuck), None);
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("old") && !fake.has("none"), "{:?}", fake.markers);
    assert_eq!(fake.asked("stop"), 0);
    // A marker the current install wrote isn't stale: it still refuses.
    fake.installed("now", Some(stuck), Some(INSTALL));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
}

#[test]
fn with_no_install_id_or_one_that_cant_be_read_no_marker_is_stale_by_it() {
    let stuck = MarkerState::AwaitingStop { stop: 900 };
    // An install from before the ids: its markers have none, and keep today's rule.
    let mut old = Fake::new();
    old.installed("a1", Some(stuck), None);
    assert!(logon_check(&mut old, &sid(POOL_A)).is_err());
    assert_eq!(old.state("a1"), Some(stuck));
    // A marker with an id while the record has none: its install was removed.
    old.installed("a2", Some(stuck), Some(OLD_INSTALL));
    drop(logon_check(&mut old, &sid(POOL_A)));
    assert!(!old.has("a2"));
    // An id that can't be read makes nothing stale.
    let mut unread = Fake::new();
    unread.install = Err("RegGetValueW failed (5)".to_owned());
    unread.installed("a3", Some(stuck), Some(OLD_INSTALL));
    assert!(logon_check(&mut unread, &sid(POOL_A)).is_err());
    assert!(unread.has("a3"));
}

#[test]
fn an_unreadable_marker_isnt_written_again_while_the_install_id_cant_be_read() {
    let mut fake = Fake::new();
    fake.install = Err("RegGetValueW failed (5)".to_owned());
    fake.marker("x1", None, None);
    // Refused, the stop requested, but the marker left unreadable: written id-less, the next
    // read would take it for another install's and remove it.
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(fake.markers["x1"], (None, None, None));
    assert_eq!(fake.asked("stop"), 1);
    assert!(start_up(&mut fake).is_ok());
    assert_eq!(fake.markers["x1"], (None, None, None));
    // Once the id reads, it is replaced under it, and isn't stale.
    fake.install = Ok(Some(INSTALL.to_owned()));
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let read = MarkerState::AwaitingStop { stop: 5 };
    assert_eq!(
        fake.markers["x1"],
        (None, Some(read), Some(INSTALL.to_owned()))
    );
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert!(fake.has("x1"), "not removed as another install's");
}
