//! The deadline's watchdog on a fake clock (`deadline.rs`), and `--deadline` on the command
//! line (`args.rs`): when the cancel and the end come, that a run that ends in time is never
//! ended, and which modes take the flag.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use super::super::args::{DEADLINE_MAX, Mode, invocation};
use super::*;

/// A clock whose waits are logged and pass at once, unless a gate holds the first one.
struct FakeClock {
    waits: Sender<Duration>,
    /// The first wait passes only once this gets a message (or its sender is dropped).
    gate: Option<Receiver<()>>,
}

impl Clock for FakeClock {
    fn wait(&mut self, span: Duration, stop: &Receiver<()>) -> Waited {
        if let Some(gate) = self.gate.take() {
            // Bounded, so a test that never opens it fails rather than hangs.
            let _opened = gate.recv_timeout(Duration::from_secs(10));
        }
        if stop.try_recv().is_ok() {
            return Waited::Stopped;
        }
        let _logged = self.waits.send(span);
        Waited::Elapsed
    }
}

/// A flag the watchdog sets, and a channel its end reports on.
fn actions() -> (Arc<AtomicBool>, impl FnOnce(), Sender<u8>, Receiver<u8>) {
    let flag = Arc::new(AtomicBool::new(false));
    let set = Arc::clone(&flag);
    let (ended, end) = mpsc::channel();
    (flag, move || set.store(true, Ordering::SeqCst), ended, end)
}

#[test]
fn the_cancel_comes_r_before_the_deadline_and_the_end_at_it() {
    let (waits, waited) = mpsc::channel();
    let (gate, held) = mpsc::channel();
    let (flag, on_cancel, ended, end) = actions();
    let clock = FakeClock {
        waits,
        gate: Some(held),
    };
    let watchdog = start(Duration::from_secs(600), clock, on_cancel, move || {
        let _sent = ended.send(11);
    })
    .unwrap();
    // Nothing yet: the first wait is held.
    assert!(!flag.load(Ordering::SeqCst));
    drop(gate);
    assert_eq!(end.recv_timeout(Duration::from_secs(10)), Ok(11));
    assert!(flag.load(Ordering::SeqCst));
    let spans: Vec<Duration> = waited.try_iter().collect();
    assert_eq!(spans, [Duration::from_secs(585), ROLLBACK_BOUND]);
    drop(watchdog);
}

#[test]
fn a_deadline_within_r_cancels_at_once() {
    for secs in [1, 10, 15] {
        let (waits, waited) = mpsc::channel();
        let (gate, held) = mpsc::channel();
        let (flag, on_cancel, ended, end) = actions();
        let clock = FakeClock {
            waits,
            gate: Some(held),
        };
        let watchdog = start(Duration::from_secs(secs), clock, on_cancel, move || {
            let _sent = ended.send(11);
        })
        .unwrap();
        // Set before `start` returns, before the main flow's first change.
        assert!(flag.load(Ordering::SeqCst), "{secs}");
        drop(gate);
        assert_eq!(end.recv_timeout(Duration::from_secs(10)), Ok(11));
        let spans: Vec<Duration> = waited.try_iter().collect();
        assert_eq!(spans, [Duration::from_secs(secs)], "the end still waits D");
        drop(watchdog);
    }
    assert_eq!(
        schedule(Duration::from_secs(16)),
        (Duration::from_secs(1), ROLLBACK_BOUND)
    );
}

#[test]
fn a_run_that_ends_in_time_is_never_cancelled_or_ended() {
    let (flag, on_cancel, ended, end) = actions();
    let watchdog = start(Duration::from_secs(3600), RealClock, on_cancel, move || {
        let _sent = ended.send(11);
    })
    .unwrap();
    drop(watchdog);
    assert!(!flag.load(Ordering::SeqCst));
    assert!(end.try_recv().is_err());
}

#[test]
fn the_real_clock_waits_its_span_or_stops() {
    let (stop, stopped) = mpsc::channel::<()>();
    let span = Duration::from_millis(20);
    assert_eq!(RealClock.wait(span, &stopped), Waited::Elapsed);
    drop(stop);
    assert_eq!(
        RealClock.wait(Duration::from_secs(60), &stopped),
        Waited::Stopped
    );
}

fn line(args: &[&str]) -> Result<Option<Duration>, String> {
    invocation(args.iter().map(OsString::from))
        .map(|run| run.deadline)
        .map_err(|usage| usage.0)
}

#[test]
fn the_deadline_is_taken_by_the_three_elevated_entries_only() {
    let set_up = ["--set-up", "--parent", "7", "--data-dir", r"C:\d"];
    let adopt = ["--adopt", "--user-install-dir", r"C:\i", "--per-machine"];
    for base in [&set_up[..], &adopt[..], &["--remove", "--uninstall"][..]] {
        assert_eq!(line(base), Ok(None), "no flag, no deadline: {base:?}");
        let mut with = base.to_vec();
        with.extend(["--deadline", "540"]);
        assert_eq!(line(&with), Ok(Some(Duration::from_secs(540))), "{base:?}");
    }
    // The mode is the one `parse` gives, the flag beside it.
    let run = invocation(
        ["--remove", "--deadline", "60", "--per-machine"]
            .iter()
            .map(OsString::from),
    )
    .unwrap();
    assert_eq!(
        run.mode,
        Mode::Remove {
            per_machine: true,
            uninstall: false
        }
    );
    let run = invocation(set_up.iter().map(OsString::from)).unwrap();
    assert_eq!(
        run.mode,
        Mode::SetUp {
            parent: 7,
            data_dir: PathBuf::from(r"C:\d"),
            add: 0
        }
    );
    for refused in [
        &["--reclaim", "--parent", "9", "--deadline", "60"][..],
        &["--refresh", "--deadline", "60"][..],
        &["--stop", "--deadline", "60"][..],
        &["--sweep-tasks", "--deadline", "60"][..],
    ] {
        assert!(line(refused).is_err(), "{refused:?}");
    }
    for bad in ["0", "-5", "+5", "1.5", "86401", "", " 60", "sixty"] {
        assert!(line(&["--remove", "--deadline", bad]).is_err(), "{bad:?}");
    }
    assert_eq!(
        line(&["--remove", "--deadline", "86400"]),
        Ok(Some(Duration::from_secs(DEADLINE_MAX)))
    );
    assert!(line(&["--remove", "--deadline"]).is_err());
    assert!(line(&["--remove", "--deadline", "5", "--deadline", "6"]).is_err());
}
