//! The git watch's end ([`EndSweep::kill_now`]) through a fake job: it returns once the job is
//! empty and says so, gives up within its bound when it isn't, and the sweeper's logon follows
//! off its path, once; and the end is known as begun before any path ends the job, which the
//! outer's start reads (`start.rs`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::*;

/// How long a gated step waits for the test before it goes on anyway.
const GATE: Duration = Duration::from_secs(5);

/// Records each step; the job's process counts come from `counts`.
#[derive(Default)]
struct Fake {
    done: Mutex<Vec<String>>,
    /// The job's process counts, in turn; the last one stays.
    counts: Mutex<VecDeque<u32>>,
    /// When set, the logon waits for a word on it.
    logon_gate: Mutex<Option<mpsc::Receiver<()>>>,
    /// When set, the job's end says it began, then waits for a word.
    end_gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl Fake {
    fn counting(counts: &[u32]) -> Fake {
        Fake {
            counts: Mutex::new(counts.iter().copied().collect()),
            ..Fake::default()
        }
    }

    fn push(&self, line: impl Into<String>) {
        self.done.lock().unwrap().push(line.into());
    }

    fn done(&self) -> Vec<String> {
        self.done.lock().unwrap().clone()
    }

    fn count(&self) -> u32 {
        let mut counts = self.counts.lock().unwrap();
        if counts.len() > 1 {
            counts.pop_front().unwrap()
        } else {
            counts.front().copied().unwrap_or(0)
        }
    }
}

impl SweeperSteps for Fake {
    type Sweeper = u32;
    fn empty_job(&self) -> Result<(), String> {
        self.push("end job");
        if let Some((began, go)) = self.end_gate.lock().unwrap().take() {
            began.send(()).unwrap();
            go.recv_timeout(GATE).unwrap();
        }
        wait_empty(
            || {
                let left = self.count();
                self.push(format!("count {left}"));
                Ok(left)
            },
            |_| self.push("pause"),
            EMPTY_WAIT,
            EMPTY_POLL,
        )
    }
    fn log_on(&self) -> Result<u32, String> {
        let gate = self.logon_gate.lock().unwrap().take();
        if let Some(go) = gate {
            go.recv_timeout(GATE).unwrap();
        }
        self.push("log on");
        Ok(9)
    }
    fn shut(&self, _: &u32) -> Result<(), String> {
        self.push("shut");
        Ok(())
    }
    fn resume(&self, _: &u32) -> Result<(), String> {
        self.push("resume");
        Ok(())
    }
    fn kill(&self, sweeper: &u32) {
        self.push(format!("kill {sweeper}"));
    }
    fn keep(&self, sweeper: u32) {
        self.push(format!("keep {sweeper}"));
    }
    fn log(&self, line: &str) {
        self.push(format!("log {line}"));
    }
    fn now(&self) -> Duration {
        Duration::ZERO
    }
    fn wait(&self) -> Swept {
        Swept::Unclean
    }
}

/// `want` as owned lines.
fn lines(want: &[&str]) -> Vec<String> {
    want.iter().map(|s| (*s).to_owned()).collect()
}

/// How many times `line` is in `done`.
fn times(done: &[String], line: &str) -> usize {
    done.iter().filter(|s| *s == line).count()
}

#[test]
fn the_watchs_kill_returns_once_the_job_is_empty_and_the_logon_follows_once() {
    let (go, logon_gate) = mpsc::channel();
    let end = Arc::new(EndSweep::new(Fake {
        logon_gate: Mutex::new(Some(logon_gate)),
        ..Fake::counting(&[2, 1, 0])
    }));
    assert!(end.kill_now(EndSignal::Failed));
    assert_eq!(end.began(), Some(EndSignal::Failed));
    // It returned once the job's count was 0, and before the sweeper's logon was over: the
    // logon still waits for `go`.
    let killed = lines(&["end job", "count 2", "pause", "count 1", "pause", "count 0"]);
    let at_return = end.steps().done();
    assert_eq!(at_return.get(..killed.len()), Some(&killed[..]));
    assert!(!at_return.iter().any(|s| s == "log on" || s == "resume"));
    go.send(()).unwrap();
    // The inner's exit (the job's end ended it) waits for the sweeper's start, then finds the end
    // begun: the sweeper starts once, its job checked empty again first.
    assert!(!end.trigger(EndSignal::InnerExited));
    let mut want = killed;
    want.extend(lines(&[
        "end job", "count 0", "log on", "shut", "resume", "keep 9",
    ]));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Failed));
}

#[test]
fn a_job_that_wont_empty_gives_false_within_the_bound_and_no_sweeper() {
    let end = Arc::new(EndSweep::new(Fake::counting(&[3])));
    assert!(!end.kill_now(EndSignal::Failed));
    let done = end.steps().done();
    // Its waits add up to no more than the bound, and it says what is left.
    let pauses = u32::try_from(times(&done, "pause")).unwrap();
    assert!(pauses > 0);
    assert!(EMPTY_POLL * pauses <= EMPTY_WAIT);
    assert!(done.iter().any(|s| s
        == "log clauseo-sandbox-run: the session's job didn't empty: 3 processes were still in \
            the job after 400 ms"));
    // The sweeper's start finds the job still not empty: no logon, as on every path.
    assert!(!end.trigger(EndSignal::InnerExited));
    let done = end.steps().done();
    assert_eq!(times(&done, "end job"), 2);
    assert_eq!(times(&done, "log on"), 0);
    assert_eq!(
        done.last().map(String::as_str),
        Some(
            "log clauseo-sandbox-run: warning: no end sweeper: 3 processes were still in the job \
             after 400 ms; the next launch's first act sweeps"
        )
    );
}

#[test]
fn the_watchs_kill_after_another_path_began_still_empties_the_job() {
    let end = Arc::new(EndSweep::new(Fake::counting(&[0])));
    assert!(end.trigger(EndSignal::InnerExited));
    assert!(end.kill_now(EndSignal::Failed));
    // The first path's signal stays; the sweeper started once, before the watch's kill.
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    assert!(!end.trigger(EndSignal::Stdin));
    let done = end.steps().done();
    assert_eq!(times(&done, "log on"), 1);
    assert_eq!(times(&done, "end job"), 2);
}

/// The outer checks the end after it put the inner in the job: an end whose job's end is still
/// to come is already seen, so the inner is never resumed outside it.
#[test]
fn the_end_is_known_as_begun_before_the_job_is_ended() {
    let (began, ending) = mpsc::channel();
    let (go, end_gate) = mpsc::channel();
    let end = Arc::new(EndSweep::new(Fake {
        end_gate: Mutex::new(Some((began, end_gate))),
        ..Fake::counting(&[0])
    }));
    std::thread::scope(|scope| {
        let watch = scope.spawn(|| end.kill_now(EndSignal::Failed));
        ending.recv_timeout(GATE).unwrap();
        assert_eq!(end.began(), Some(EndSignal::Failed));
        assert_eq!(end.steps().done(), ["end job"]);
        go.send(()).unwrap();
        assert!(watch.join().unwrap());
    });
    // The same on the other paths, through `trigger`.
    let (began, ending) = mpsc::channel();
    let (go, end_gate) = mpsc::channel();
    let end = EndSweep::new(Fake {
        end_gate: Mutex::new(Some((began, end_gate))),
        ..Fake::counting(&[0])
    });
    std::thread::scope(|scope| {
        let failed = scope.spawn(|| end.trigger(EndSignal::Failed));
        ending.recv_timeout(GATE).unwrap();
        assert_eq!(end.began(), Some(EndSignal::Failed));
        go.send(()).unwrap();
        assert!(failed.join().unwrap());
    });
}

/// A session ended before its agent ran exits as one the job's end ended would have.
#[test]
fn a_session_ended_before_its_start_exits_as_the_ends_own_outcome() {
    assert_eq!(
        ended_before_start(EndSignal::Failed),
        exit_code(Some(EndSignal::Failed), SWEPT, None)
    );
    assert_eq!(
        ended_before_start(EndSignal::Failed),
        RunExit::AgentReserved.code()
    );
}
