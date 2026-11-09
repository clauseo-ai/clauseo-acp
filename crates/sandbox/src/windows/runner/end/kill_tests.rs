//! The git watch's end ([`SessionEnd::kill_now`]) through a fake job: it returns once the job is
//! empty and says so, gives up within its bound when it isn't, and the sweep task's request
//! follows off its path, once; and the end is known as begun before any path ends the job, which
//! the outer's start reads (`start.rs`).

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
    /// When set, the sweep task's request waits for a word on it.
    request_gate: Mutex<Option<mpsc::Receiver<()>>>,
    /// Whether the sweep task's request fails.
    request_fails: bool,
    /// When set, the job's end says it began, then waits for a word.
    end_gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    /// How many of the sweep task's requests panic, the first ones.
    request_panics: Mutex<u32>,
    /// When set, each logged line is sent on it too.
    logged: Mutex<Option<mpsc::Sender<String>>>,
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

impl EndSteps for Fake {
    fn empty_job(&self) -> Result<(), Detail> {
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
        .map_err(|what| Detail::new(|| what))
    }
    fn next_number(&self, name: &'static str) -> Result<u64, Detail> {
        self.push(format!("read {name}"));
        Ok(SWEEP_AT)
    }
    fn request_sweep(&self) -> Result<(), Detail> {
        let panics = {
            let mut left = self.request_panics.lock().unwrap();
            let panics = *left > 0;
            *left = left.saturating_sub(1);
            panics
        };
        assert!(!panics, "the fake's request panics");
        let gate = self.request_gate.lock().unwrap().take();
        if let Some(go) = gate {
            go.recv_timeout(GATE).unwrap();
        }
        self.push("request sweep");
        if self.request_fails {
            return Err("request sweep failed (5)".into());
        }
        Ok(())
    }
    fn request_stop(&self) -> bool {
        self.push("request stop");
        true
    }
    fn settle(&self, state: &MarkerState) -> Result<(), Detail> {
        self.push(format!("settle {state:?}"));
        Ok(())
    }
    fn log(&self, _kind: RunnerKind, detail: Detail) {
        let line = detail.text();
        self.push(format!("log {line}"));
        if let Some(logged) = &*self.logged.lock().unwrap() {
            drop(logged.send(line.to_owned()));
        }
    }
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// The fake's `SweepNext`.
const SWEEP_AT: u64 = 12;

/// `want` as owned lines.
fn lines(want: &[&str]) -> Vec<String> {
    want.iter().map(|s| (*s).to_owned()).collect()
}

/// How many times `line` is in `done`.
fn times(done: &[String], line: &str) -> usize {
    done.iter().filter(|s| *s == line).count()
}

#[test]
fn the_watchs_kill_returns_once_the_job_is_empty_and_the_request_follows_once() {
    let (go, request_gate) = mpsc::channel();
    let end = Arc::new(SessionEnd::new(Fake {
        request_gate: Mutex::new(Some(request_gate)),
        ..Fake::counting(&[2, 1, 0])
    }));
    assert!(end.kill_now(EndSignal::Failed));
    assert_eq!(end.began(), Some(EndSignal::Failed));
    // It returned once the job's count was 0, and before the sweep task's request was over: the
    // request still waits for `go`.
    let killed = lines(&["end job", "count 2", "pause", "count 1", "pause", "count 0"]);
    let at_return = end.steps().done();
    assert_eq!(at_return.get(..killed.len()), Some(&killed[..]));
    assert!(!at_return.iter().any(|s| s == "request sweep"));
    go.send(()).unwrap();
    // The inner's exit (the job's end ended it) waits for the request, then finds the end begun:
    // the sweep task is requested once, its job checked empty again first.
    assert!(!end.trigger(EndSignal::InnerExited));
    let mut want = killed;
    want.extend(lines(&[
        "end job",
        "count 0",
        "read SweepNext",
        "request sweep",
    ]));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Failed));
}

#[test]
fn a_job_that_wont_empty_gives_false_within_the_bound_and_the_request_follows() {
    let end = Arc::new(SessionEnd::new(Fake::counting(&[3])));
    assert!(!end.kill_now(EndSignal::Failed));
    let done = end.steps().done();
    // Its waits add up to no more than the bound, and it says what is left.
    let pauses = u32::try_from(times(&done, "pause")).unwrap();
    assert!(pauses > 0);
    assert!(EMPTY_POLL * pauses <= EMPTY_WAIT);
    assert!(done.iter().any(|s| s
        == "log clauseo-sandbox-run: the session's job didn't empty: 3 processes were still in \
            the job after 400 ms"));
    // The session's end finds the job still not empty, and requests the sweep task all the same,
    // as on every path.
    assert!(!end.trigger(EndSignal::InnerExited));
    let done = end.steps().done();
    assert_eq!(times(&done, "end job"), 2);
    assert_eq!(times(&done, "request sweep"), 1);
    assert!(done.iter().any(|s| s
        == "log clauseo-sandbox-run: warning: the job didn't empty (3 processes were still in the \
            job after 400 ms); the sandbox sweep task is requested all the same"));
    assert_eq!(done.last().map(String::as_str), Some("request sweep"));
}

#[test]
fn the_watchs_kill_after_another_path_began_still_empties_the_job() {
    let end = Arc::new(SessionEnd::new(Fake::counting(&[0])));
    assert!(end.trigger(EndSignal::InnerExited));
    assert!(end.kill_now(EndSignal::Failed));
    // The first path's signal stays; the sweep task was requested once, before the watch's kill.
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    assert!(!end.trigger(EndSignal::Stdin));
    let done = end.steps().done();
    assert_eq!(times(&done, "request sweep"), 1);
    assert_eq!(times(&done, "end job"), 2);
}

/// The outer checks the end after it put the inner in the job: an end whose job's end is still
/// to come is already seen, so the inner is never resumed outside it.
#[test]
fn the_end_is_known_as_begun_before_the_job_is_ended() {
    let (began, ending) = mpsc::channel();
    let (go, end_gate) = mpsc::channel();
    let end = Arc::new(SessionEnd::new(Fake {
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
    let end = SessionEnd::new(Fake {
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

/// The watch's kill racing the inner's exit: the sweep task is requested once, whichever comes
/// first.
#[test]
fn the_request_happens_once_when_the_end_races_the_watchs_kill() {
    for _ in 0..20 {
        let end = Arc::new(SessionEnd::new(Fake::counting(&[0])));
        std::thread::scope(|scope| {
            let watch = scope.spawn(|| end.kill_now(EndSignal::Failed));
            let exited = scope.spawn(|| end.trigger(EndSignal::InnerExited));
            assert!(watch.join().unwrap());
            exited.join().unwrap();
        });
        // The watch's request thread may still be going: a trigger waits for it.
        assert!(!end.trigger(EndSignal::Stdin));
        let done = end.steps().done();
        assert_eq!(times(&done, "request sweep"), 1);
        assert_eq!(times(&done, "read SweepNext"), 1);
    }
}

/// A request that fails after the watch's kill is logged as on every path, and the marker still
/// awaits the sweep `SweepNext` named; the exit code is unchanged.
#[test]
fn a_failed_request_after_the_watchs_kill_is_logged_and_the_marker_still_awaits_the_sweep() {
    let end = Arc::new(SessionEnd::new(Fake {
        request_fails: true,
        ..Fake::counting(&[0])
    }));
    assert!(end.kill_now(EndSignal::Failed));
    assert!(!end.trigger(EndSignal::InnerExited));
    end.finish();
    let done = end.steps().done();
    assert_eq!(times(&done, "request sweep"), 1);
    assert!(done.iter().any(|s| s
        == "log clauseo-sandbox-run: warning: the sandbox sweep task couldn't be requested \
            (request sweep failed (5))"));
    assert_eq!(
        done.last().map(String::as_str),
        Some("settle AwaitingSweep { sweep: 12 }")
    );
    assert_eq!(end.outcome(0), 0);
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

/// A panic in the end on the watch's `session-end` thread is said, by its kind alone, and the
/// exit's trigger runs the end again: the sweep task is still requested.
#[test]
fn a_panic_in_the_ends_own_thread_is_logged_and_the_exit_ends_again() {
    let (logged, lines) = mpsc::channel();
    let end = Arc::new(SessionEnd::new(Fake {
        request_panics: Mutex::new(1),
        logged: Mutex::new(Some(logged)),
        ..Fake::counting(&[0])
    }));
    assert!(end.kill_now(EndSignal::Failed));
    let said =
        std::iter::from_fn(|| lines.recv_timeout(GATE).ok()).find(|line| line == END_PANICKED);
    assert_eq!(said.as_deref(), Some(END_PANICKED));
    assert!(!end.trigger(EndSignal::InnerExited));
    let done = end.steps().done();
    assert_eq!(times(&done, "request sweep"), 1);
}
