//! Review item 2: the watchdog holds the budget without the check's help. A check blocked in a
//! call (an open an agent's oplock holds up) ends the session from the watchdog's own thread
//! while the watch's thread is still blocked; a check that stays within the budget, or one
//! running once the session ended, ends nothing.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use super::super::watchdog::TICK;
use super::super::{Changes, Dog, Guard, Live, Pulse, Session, watch, watchdog};
use super::{added, script};
use crate::windows::set_aside::{Failed, Hooks, SCAN_BUDGET};

/// A monotonic clock both threads read, in milliseconds.
#[derive(Default)]
struct Clock(AtomicU64);

impl Clock {
    fn read(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }

    fn advance(&self, by: Duration) -> Duration {
        let by = u64::try_from(by.as_millis()).unwrap();
        Duration::from_millis(self.0.fetch_add(by, Ordering::SeqCst) + by)
    }
}

/// A guard whose check blocks, as an open held up by an oplock would, until it is released.
struct Blocked {
    release: Mutex<mpsc::Receiver<()>>,
    returned: Arc<AtomicBool>,
}

impl Guard for Blocked {
    fn changed(&self, _changes: &Changes, _hooks: &impl Hooks) -> Live {
        self.release.lock().unwrap().recv().unwrap();
        self.returned.store(true, Ordering::SeqCst);
        Live::default()
    }

    fn rescan(&self, _hooks: &impl Hooks) -> Live {
        Live::default()
    }

    fn retry(&self, _failed: &[Failed], _hooks: &impl Hooks) -> Live {
        Live::default()
    }
}

/// The watch's side: the shared clock and pulse.
struct Watching {
    clock: Arc<Clock>,
    pulse: Arc<Pulse>,
}

impl Session for Watching {
    fn end(&self) -> bool {
        true
    }

    fn say(&self, _line: &str) {}

    fn pause(&self, _wait: Duration) -> bool {
        true
    }

    fn settle(&self, _wait: Duration) {}

    fn now(&self) -> Duration {
        self.clock.read()
    }

    fn pulse(&self) -> &Pulse {
        &self.pulse
    }
}

/// The watchdog's side: each reading moves the clock half a second; each wait is short, real,
/// and ends early once stopped; a stall is sent on.
struct Watchdog {
    clock: Arc<Clock>,
    stop: Mutex<mpsc::Receiver<()>>,
    stalled: mpsc::Sender<bool>,
}

impl Dog for Watchdog {
    fn wait(&self, wait: Duration) -> bool {
        assert_eq!(wait, TICK);
        let stop = self.stop.lock().unwrap();
        !matches!(
            stop.recv_timeout(Duration::from_millis(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        )
    }

    fn now(&self) -> Duration {
        self.clock.advance(Duration::from_millis(500))
    }

    fn stall(&self, first: bool) {
        self.stalled.send(first).unwrap();
    }
}

#[test]
fn a_check_blocked_past_the_budget_ends_the_session_while_the_watch_is_still_blocked() {
    let (clock, pulse) = (Arc::new(Clock::default()), Arc::new(Pulse::default()));
    let (release, released) = mpsc::channel();
    let returned = Arc::new(AtomicBool::new(false));
    let (watch_clock, watch_pulse, watch_returned) = (
        Arc::clone(&clock),
        Arc::clone(&pulse),
        Arc::clone(&returned),
    );
    let watcher = thread::spawn(move || {
        let guard = Blocked {
            release: Mutex::new(released),
            returned: watch_returned,
        };
        let session = Watching {
            clock: watch_clock,
            pulse: watch_pulse,
        };
        watch(script(vec![added(&["a/HEAD"])]), &guard, &session);
    });
    let (stalled, heard) = mpsc::channel();
    let (_stop, stopped) = mpsc::channel::<()>();
    let (dog_clock, dog_pulse) = (Arc::clone(&clock), Arc::clone(&pulse));
    let dog = thread::spawn(move || {
        let dog = Watchdog {
            clock: dog_clock,
            stop: Mutex::new(stopped),
            stalled,
        };
        watchdog(&dog_pulse, &dog);
    });

    let first = heard.recv_timeout(Duration::from_secs(30));
    assert_eq!(first, Ok(true), "the watchdog ended the session, first");
    assert!(
        !returned.load(Ordering::SeqCst),
        "the watch's thread is still blocked"
    );
    assert!(clock.read() > SCAN_BUDGET);
    dog.join().unwrap();
    release.send(()).unwrap();
    watcher.join().unwrap();
    assert!(returned.load(Ordering::SeqCst));
}

#[test]
fn a_check_within_the_budget_or_after_the_end_never_stalls() {
    let pulse = Pulse::default();
    assert!(!pulse.stalled(Duration::from_secs(60), SCAN_BUDGET), "idle");
    pulse.busy(Duration::from_secs(10));
    assert!(!pulse.stalled(Duration::from_secs(12), SCAN_BUDGET));
    assert!(pulse.stalled(Duration::from_millis(12_001), SCAN_BUDGET));
    pulse.idle();
    assert!(!pulse.stalled(Duration::from_secs(60), SCAN_BUDGET));
    pulse.busy(Duration::ZERO);
    assert!(pulse.end(), "first to end");
    assert!(!pulse.end());
    assert!(
        !pulse.stalled(Duration::from_secs(60), SCAN_BUDGET),
        "the session ended: nothing alive to stall it"
    );
}

/// The watchdog stops with the watch, ending nothing.
#[test]
fn a_stopped_watchdog_ends_nothing() {
    let (stalled, heard) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    stop.send(()).unwrap();
    let dog = Watchdog {
        clock: Arc::new(Clock::default()),
        stop: Mutex::new(stopped),
        stalled,
    };
    let pulse = Pulse::default();
    pulse.busy(Duration::ZERO);
    watchdog(&pulse, &dog);
    drop(dog);
    assert_eq!(heard.recv(), Err(mpsc::RecvError));
}
