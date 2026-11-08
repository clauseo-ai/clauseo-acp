//! The markers' checks through a fake, on every OS: F4 refusing a logon until `StopSequence`
//! reaches a stop-awaiting marker's number, a dead outer's marker made one awaiting a stop, a
//! launch's bounded wait for a sweep, and the desktop's start-up requests.

use std::collections::BTreeMap;

use super::*;

const POOL_A: &str = "S-1-5-21-1-2-3-1010";
const POOL_B: &str = "S-1-5-21-1-2-3-1011";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// The fake: the markers by id, the record's numbers, and everything done.
struct Fake {
    markers: BTreeMap<String, (Option<Sid>, Option<MarkerState>)>,
    done: Sequences,
    /// What each pause moves `done.sweep` to, in turn: a sweep run completing.
    sweeps: Vec<u64>,
    stop_next: u64,
    sweep_next: u64,
    pool: Vec<Sid>,
    unreadable: bool,
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
            unreadable: false,
            log: Vec::new(),
        }
    }

    fn marker(&mut self, id: &str, user: Option<&str>, state: Option<MarkerState>) {
        self.markers.insert(id.to_owned(), (user.map(sid), state));
    }

    /// The marker's state; `None` when it is gone or unreadable.
    fn state(&self, id: &str) -> Option<MarkerState> {
        self.markers.get(id).and_then(|(_, state)| *state)
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
            .map(|(id, (user, state))| UncleanSession {
                id: id.clone(),
                pool_user: user.clone(),
                state: *state,
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
        Ok(if name == STOP_NEXT {
            self.stop_next
        } else {
            self.sweep_next
        })
    }
    fn pool(&mut self) -> Vec<Sid> {
        self.pool.clone()
    }
    fn settle(&mut self, id: &str, pool_user: Option<&Sid>, state: MarkerState) -> io::Result<()> {
        self.markers
            .insert(id.to_owned(), (pool_user.cloned(), Some(state)));
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

const STOP: MarkerState = MarkerState::AwaitingStop { stop: 3, sweep: 2 };

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
    // Once the stop that reserved 3 completed, and the sweep too: the logon goes, the marker goes.
    fake.done = Sequences { stop: 3, sweep: 2 };
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert!(!fake.has("a1"));
    assert_eq!(fake.asked("stop"), 2);
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
    // A marker awaiting only a sweep is the launch's to wait for, not F4's.
    fake.marker(
        "a2",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 40 }),
    );
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    // A stop met but a sweep not: the logon goes, and the marker stays for the launch.
    fake.marker("a3", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 1 };
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert_eq!(fake.state("a3"), Some(STOP));
    // Sequences that can't be read refuse.
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
    let read = MarkerState::AwaitingStop { stop: 5, sweep: 9 };
    assert_eq!(fake.state("a1"), Some(read));
    assert_eq!(fake.log, ["read StopNext", "read SweepNext", "stop"]);
    // The stop that reserved 5 completed, and a sweep past 9: the next logon goes.
    fake.done = Sequences { stop: 5, sweep: 9 };
    fake.done.stop = 4;
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    fake.done.stop = 5;
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
fn a_stop_awaiting_marker_holds_a_launch_only_for_its_sweep() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 0, sweep: 2 };
    // Its sweep is met; its stop is F4's, so the launch goes and the marker stays.
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.state("a1"), Some(STOP));
    // Sequences that can't be read refuse.
    fake.unreadable = true;
    assert!(await_sweeps(&mut fake).is_err());
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
    let found = start_up(&mut fake).unwrap();
    let read = MarkerState::AwaitingStop { stop: 5, sweep: 9 };
    assert_eq!(fake.state("a1"), Some(read));
    // An unreadable one is written again, naming no pool user, so it counts for all.
    assert_eq!(fake.markers["x1"], (None, Some(read)));
    assert!(!fake.has("b1"), "a clean one goes");
    let want = StartUp {
        left: 2,
        stop_requested: true,
        sweep_requested: true,
    };
    assert_eq!(found, want);
    // Each number read once; it never waits.
    assert_eq!(fake.asked("read StopNext"), 1);
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
