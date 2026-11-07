//! The end sweeper's start through a recording fake: the job emptied first, no sweeper while it
//! isn't or once too little of the host's 2 s after its stdin end is left, once a session on every
//! end path, never in the job, and the host's stdin end set apart from the agent's; and the exit
//! code on a cancel.

use std::collections::VecDeque;
use std::io;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::windows::runner::sweep::SWEPT;

/// Records each step; fails the one named `fail`.
#[derive(Default)]
struct Record {
    done: Mutex<Vec<String>>,
    fail: Option<&'static str>,
    /// Told when the logon begins, which then takes a while.
    slow: Mutex<Option<mpsc::Sender<()>>>,
    /// How the sweeper ended, for the outer's wait; `Unclean` (none kept) when unset.
    swept: Option<Swept>,
    /// The clock's readings, in turn; 0 once they run out.
    readings: Mutex<VecDeque<Duration>>,
}

impl Record {
    fn step(&self, name: &str) -> Result<(), String> {
        self.done.lock().unwrap().push(name.to_owned());
        if self.fail == Some(name) {
            return Err(format!("{name} failed (5)"));
        }
        Ok(())
    }

    fn done(&self) -> Vec<String> {
        self.done.lock().unwrap().clone()
    }
}

impl SweeperSteps for Record {
    type Sweeper = u32;
    fn empty_job(&self) -> Result<(), String> {
        self.step("empty job")
    }
    fn log_on(&self) -> Result<u32, String> {
        if let Some(begun) = self.slow.lock().unwrap().take() {
            begun.send(()).unwrap();
            // A logon that takes 100 ms: nothing is ever sent on `idle`.
            let (_kept, idle) = mpsc::channel::<()>();
            assert!(idle.recv_timeout(Duration::from_millis(100)).is_err());
        }
        self.step("log on").map(|()| 9)
    }
    fn shut(&self, _: &u32) -> Result<(), String> {
        self.step("shut")
    }
    fn resume(&self, _: &u32) -> Result<(), String> {
        self.step("resume")
    }
    fn kill(&self, sweeper: &u32) {
        self.done.lock().unwrap().push(format!("kill {sweeper}"));
    }
    fn keep(&self, sweeper: u32) {
        self.done.lock().unwrap().push(format!("keep {sweeper}"));
    }
    fn log(&self, line: &str) {
        self.done.lock().unwrap().push(format!("log {line}"));
    }
    fn now(&self) -> Duration {
        self.readings
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
    }
    fn wait(&self) -> Swept {
        self.swept.unwrap_or(Swept::Unclean)
    }
}

const STARTED: [&str; 5] = ["empty job", "log on", "shut", "resume", "keep 9"];
const NOT_EMPTY: [&str; 2] = [
    "empty job",
    "log clauseo-sandbox-run: warning: no end sweeper: empty job failed (5); the next launch's \
     first act sweeps",
];

#[test]
fn the_job_is_emptied_then_the_sweeper_is_logged_on_shut_and_resumed_and_never_put_in_the_job() {
    let record = Record::default();
    assert!(start_sweeper(&record, None));
    // The job is empty before the logon; no step puts the sweeper in a job: the host ending the
    // outer's job leaves it running.
    assert_eq!(record.done(), STARTED);
}

#[test]
fn a_job_that_doesnt_empty_starts_no_sweeper_on_any_path() {
    let not_empty = || Record {
        fail: Some("empty job"),
        ..Record::default()
    };
    let record = not_empty();
    assert!(!start_sweeper(&record, None));
    assert_eq!(record.done(), NOT_EMPTY);
    for path in [EndSignal::InnerExited, EndSignal::Failed] {
        let end = EndSweep::new(not_empty());
        assert!(end.trigger(path), "{path:?}");
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
        assert_eq!(end.began(), Some(path));
        // A later path doesn't try again: the next launch's first act is the backstop.
        assert!(!end.trigger(EndSignal::Stdin));
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
    }
    // The host's stdin end: the agent's stdin closes, its grace runs out, and no sweeper.
    let end = EndSweep::new(not_empty());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps().log("agent stdin closed");
    });
    let mut want = vec!["log agent stdin closed".to_owned()];
    want.extend(NOT_EMPTY.iter().map(|s| (*s).to_owned()));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
}

#[test]
fn a_path_during_another_paths_start_returns_once_it_is_over() {
    let (begun, logging_on) = mpsc::channel();
    let end = EndSweep::new(Record {
        slow: Mutex::new(Some(begun)),
        ..Record::default()
    });
    std::thread::scope(|scope| {
        let stdin = scope.spawn(|| end.trigger(EndSignal::Stdin));
        logging_on.recv().unwrap();
        // The inner exited (the job's end ended it) while the stdin path is still logging on:
        // this path waits, so the outer never exits with the sweeper half started.
        assert!(!end.trigger(EndSignal::InnerExited));
        assert_eq!(end.steps().done(), STARTED);
        assert!(stdin.join().unwrap());
    });
    assert_eq!(end.began(), Some(EndSignal::Stdin));
}

#[test]
fn the_wait_for_an_empty_job_polls_until_none_is_left_and_no_longer() {
    let poll = Duration::from_millis(25);
    let wait = Duration::from_millis(100);
    let mut counts = vec![3, 1, 0].into_iter();
    let mut pauses = Vec::new();
    let got = wait_empty(
        || Ok(counts.next().unwrap()),
        |d| pauses.push(d),
        wait,
        poll,
    );
    assert_eq!(got, Ok(()));
    assert_eq!(pauses, [poll, poll]);
    // Never empty: four pauses fit the wait, five counts are asked, and the last one is named.
    let mut asked = 0;
    let mut pauses = 0;
    let got = wait_empty(
        || {
            asked += 1;
            Ok(2)
        },
        |_| pauses += 1,
        wait,
        poll,
    );
    assert_eq!(
        got,
        Err("2 processes were still in the job after 100 ms".to_owned())
    );
    assert_eq!((asked, pauses), (5, 4));
    // A count that can't be read is an error too: no sweeper.
    let got = wait_empty(|| Err(io::Error::from_raw_os_error(6)), |_| {}, wait, poll);
    assert_eq!(got, Err("the job's process count failed (6)".to_owned()));
    // The outer's own wait fits what the grace leaves of the host's 2 s.
    assert!(EMPTY_WAIT <= Duration::from_millis(400));
    assert!(EMPTY_POLL < EMPTY_WAIT);
}

#[test]
fn a_sweeper_that_fails_to_start_is_logged_and_the_run_goes_on() {
    let record = Record {
        fail: Some("log on"),
        ..Record::default()
    };
    assert!(!start_sweeper(&record, None));
    assert_eq!(
        record.done(),
        [
            "empty job",
            "log on",
            "log clauseo-sandbox-run: the end sweeper didn't start: log on failed (5)"
        ]
    );
    let record = Record {
        fail: Some("shut"),
        ..Record::default()
    };
    assert!(!start_sweeper(&record, None));
    assert_eq!(
        record.done(),
        [
            "empty job",
            "log on",
            "shut",
            "kill 9",
            "log clauseo-sandbox-run: the end sweeper didn't start: shut failed (5)"
        ]
    );
}

#[test]
fn each_end_path_starts_the_sweeper_once_whichever_comes_first() {
    let paths = [EndSignal::Stdin, EndSignal::InnerExited, EndSignal::Failed];
    for first in paths {
        let end = EndSweep::new(Record::default());
        assert!(end.trigger(first), "{first:?}");
        for later in paths {
            assert!(!end.trigger(later), "{first:?} then {later:?}");
        }
        assert_eq!(end.steps().done(), STARTED, "{first:?}");
        assert_eq!(end.began(), Some(first));
    }
    assert_eq!(EndSweep::new(Record::default()).began(), None);
}

#[test]
fn paths_on_two_threads_start_it_once() {
    let end = EndSweep::new(Record::default());
    let started: Vec<bool> = std::thread::scope(|scope| {
        let stdin = scope.spawn(|| end.trigger(EndSignal::Stdin));
        let exited = scope.spawn(|| end.trigger(EndSignal::InnerExited));
        vec![stdin.join().unwrap(), exited.join().unwrap()]
    });
    assert_eq!(started.iter().filter(|s| **s).count(), 1);
    assert_eq!(end.steps().done(), STARTED);
}

/// A grace the tests can wait out.
const SHORT: Duration = Duration::from_millis(50);

/// `want` as owned lines.
fn lines(want: &[&str]) -> Vec<String> {
    want.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn stdin_ending_with_the_agent_finishing_in_its_grace_keeps_its_code_and_output() {
    // The agent's stdin closes first; the agent finishes and the inner exits within the grace.
    // Its output drains, then the job ends and empties, then the sweeper starts.
    let end = EndSweep::new(Record::default());
    let (closed, agent_reads_eof) = mpsc::channel();
    let mut agent = Vec::new();
    let mut output = Vec::new();
    let code = std::thread::scope(|scope| {
        let stdin = scope.spawn(|| {
            relay_stdin(
                &b"request\n"[..],
                &mut agent,
                &end,
                Duration::from_secs(30),
                |_| {
                    end.steps().log("agent stdin closed");
                    closed.send(()).unwrap();
                },
            );
        });
        agent_reads_eof.recv().unwrap();
        // The agent wrote its answer and exited 3; the inner reported it and exited.
        let code = inner_exited(&end, Some(3), Some(3), || {
            end.steps().log("drain");
            output.push("answer");
        });
        // The stdin path no longer waits out its grace once the end began.
        stdin.join().unwrap();
        code
    });
    assert_eq!(code, 3);
    assert_eq!(agent, b"request\n");
    assert_eq!(output, ["answer", "answer"]);
    let mut want = lines(&["log agent stdin closed", "log drain"]);
    want.extend(lines(&STARTED));
    want.push("log drain".to_owned());
    assert_eq!(end.steps().done(), want);
    // Its end began at the inner's exit: no cancel, and a clean end when the sweeper went.
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    assert!(end.clean(Swept::Clean, true));
}

#[test]
fn stdin_ending_with_the_agent_not_finishing_ends_the_job_before_the_sweeper_in_budget() {
    let end = EndSweep::new(Record::default());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps().log("agent stdin closed");
    });
    // The grace ran out: the job ended and emptied, then the sweeper started, all on this path.
    let mut want = lines(&["log agent stdin closed"]);
    want.extend(lines(&STARTED));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
    // The job's end ended the inner: the host's job-end code, no second start, and no clean end.
    let code = inner_exited(&end, Some(SWEPT), None, || end.steps().log("drain"));
    assert_eq!(code, HOST_ENDED);
    want.extend(lines(&["log drain", "log drain"]));
    assert_eq!(end.steps().done(), want);
    assert!(!end.clean(Swept::Clean, false));
}

/// A recording fake whose clock reads `readings` in turn.
fn clocked(readings: &[Duration]) -> Record {
    Record {
        readings: Mutex::new(readings.iter().copied().collect()),
        ..Record::default()
    }
}

/// The skip's lines once `left_ms` of the host's 2 s were left.
fn skipped(left_ms: u32) -> Vec<String> {
    lines(&[
        "empty job",
        &format!(
            "log clauseo-sandbox-run: warning: no end sweeper: skipped for time: {left_ms} ms of \
             the host's 2000 ms after the stdin end were left, under the 500 ms the sweeper's \
             start needs; the next launch's first act sweeps"
        ),
    ])
}

#[test]
fn the_grace_the_jobs_end_and_the_sweepers_start_fit_the_hosts_two_seconds_with_headroom() {
    // What the host gives the outer after it closed its stdin, before it ends the outer's job.
    assert_eq!(HOST_BUDGET, Duration::from_secs(2));
    assert_eq!(GRACE, Duration::from_secs(1));
    assert_eq!(EMPTY_WAIT, Duration::from_millis(400));
    assert!(SWEEPER_BUDGET >= Duration::from_millis(500));
    // A full grace, a full wait for the job to empty, the sweeper's whole budget, and a margin
    // for the last poll and the job's end: all inside the 2 s.
    let headroom = Duration::from_millis(100);
    let used = GRACE + EMPTY_WAIT + SWEEPER_BUDGET;
    assert!(used + headroom <= HOST_BUDGET, "{used:?}");
    assert!(EMPTY_POLL < headroom);
    // So in the normal timing the sweeper starts on the stdin path.
    assert!(sweeper_fits(GRACE + EMPTY_WAIT));
}

#[test]
fn the_sweeper_starts_only_while_its_budget_of_the_hosts_two_seconds_is_left() {
    let last = HOST_BUDGET.checked_sub(SWEEPER_BUDGET).unwrap();
    assert!(sweeper_fits(Duration::ZERO));
    // Exactly its budget left: it starts.
    assert!(sweeper_fits(last));
    // Any less: skipped.
    assert!(!sweeper_fits(last + Duration::from_nanos(1)));
    assert!(!sweeper_fits(HOST_BUDGET));
    assert!(!sweeper_fits(HOST_BUDGET * 3));
}

#[test]
fn a_sweeper_too_late_after_the_stdin_end_is_skipped_and_the_session_isnt_clean() {
    let last = HOST_BUDGET.checked_sub(SWEEPER_BUDGET).unwrap();
    let ms = Duration::from_millis(1);
    // The stdin end is read at 0, the clock again once the job emptied.
    let end = EndSweep::new(clocked(&[Duration::ZERO, last]));
    relay_stdin(&b""[..], io::sink(), &end, SHORT, drop);
    assert_eq!(end.steps().done(), lines(&STARTED));
    let end = EndSweep::new(clocked(&[Duration::ZERO, last + ms]));
    relay_stdin(&b""[..], io::sink(), &end, SHORT, drop);
    // The job still ended and emptied first; then no logon at all: no sweeper to leave suspended.
    assert_eq!(end.steps().done(), skipped(499));
    assert_eq!(end.began(), Some(EndSignal::Stdin));
    assert!(!end.trigger(EndSignal::InnerExited));
    assert!(!end.clean(end.wait(), true));
    // The clock counts from the stdin end, not from its origin.
    let late = Duration::from_secs(100);
    let end = EndSweep::new(clocked(&[late, late + last + ms]));
    end.stdin_ended();
    assert!(end.trigger(EndSignal::Stdin));
    assert_eq!(end.steps().done(), skipped(499));
}

#[test]
fn the_stdin_ends_deadline_holds_on_the_inners_exit_too() {
    // The agent finished in its grace, but its output's drain took the rest: the inner's exit
    // starts no sweeper either, and the session isn't clean.
    let end = EndSweep::new(clocked(&[Duration::ZERO, Duration::from_millis(1800)]));
    end.stdin_ended();
    assert_eq!(inner_exited(&end, Some(0), Some(0), || {}), 0);
    assert_eq!(end.steps().done(), skipped(200));
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    assert!(!end.clean(end.wait(), true));
    // With no stdin end, the clock isn't read and the sweeper starts whenever the inner exits.
    let end = EndSweep::new(clocked(&[Duration::ZERO, HOST_BUDGET * 3]));
    assert_eq!(inner_exited(&end, Some(0), Some(0), || {}), 0);
    assert_eq!(end.steps().done(), lines(&STARTED));
    // Only the first stdin end counts: a second one reads no clock and moves nothing.
    let last = HOST_BUDGET.checked_sub(SWEEPER_BUDGET).unwrap();
    let ms = Duration::from_millis(1);
    let end = EndSweep::new(clocked(&[Duration::ZERO, last + ms, last + ms * 2]));
    end.stdin_ended();
    end.stdin_ended();
    assert!(end.trigger(EndSignal::Stdin));
    assert_eq!(end.steps().done(), skipped(499));
}

#[test]
fn the_inners_exit_drains_then_ends_the_job_with_no_grace() {
    let end = EndSweep::new(Record::default());
    let code = inner_exited(&end, Some(7), Some(7), || end.steps().log("drain"));
    assert_eq!(code, 7);
    let mut want = lines(&["log drain"]);
    want.extend(lines(&STARTED));
    want.push("log drain".to_owned());
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    // A wait that failed: Spawn, the sweeper started all the same.
    let end = EndSweep::new(Record::default());
    let spawn = crate::windows::runner::RunExit::Spawn.code();
    assert_eq!(inner_exited(&end, None, None, || {}), spawn);
    assert_eq!(end.steps().done(), lines(&STARTED));
    // The host's stdin ending after that waits no grace and starts nothing more.
    relay_stdin(&b""[..], io::sink(), &end, Duration::from_secs(30), drop);
    assert_eq!(end.steps().done(), lines(&STARTED));
}

/// A destination that refuses every write: an agent that closed its stdin.
struct Closed;

impl io::Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_agent_closing_its_stdin_starts_no_sweeper() {
    let end = EndSweep::new(Record::default());
    relay_stdin(&b"request\n"[..], Closed, &end, SHORT, drop);
    assert_eq!(end.steps().done(), Vec::<String>::new());
    assert_eq!(copy(&b""[..], io::sink()), CopyEnd::Source);
    assert_eq!(copy(&b"x"[..], Closed), CopyEnd::Destination);
}

#[test]
fn a_cancel_the_jobs_end_ended_exits_as_it_would_have_without_it() {
    let reserved = crate::windows::runner::RunExit::AgentReserved.code();
    let stdin = Some(EndSignal::Stdin);
    // The agent exited on its own first and the inner reported it, then the job's end (or the
    // sweeper) ended the inner: the agent's code.
    assert_eq!(exit_code(stdin, SWEPT, Some(0)), 0);
    assert_eq!(exit_code(stdin, SWEPT, Some(3)), 3);
    // ... passed on as any agent code is: never one in the runner's own range.
    assert_eq!(exit_code(stdin, SWEPT, Some(SWEPT - 1)), reserved);
    // The job's end ended the agent, or the inner before any report: the host's job-end code.
    assert_eq!(exit_code(stdin, SWEPT, Some(SWEPT)), HOST_ENDED);
    assert_eq!(exit_code(stdin, SWEPT, None), HOST_ENDED);
    assert_eq!(HOST_ENDED, 0xC1A0);
    // A cancel the inner outlived: its code, as ever.
    assert_eq!(exit_code(stdin, 0, Some(0)), 0);
}

#[test]
fn past_a_cancel_the_inners_code_passes_as_the_agents() {
    let reserved = crate::windows::runner::RunExit::AgentReserved.code();
    for began in [None, Some(EndSignal::InnerExited), Some(EndSignal::Failed)] {
        assert_eq!(exit_code(began, 7, Some(7)), 7, "{began:?}");
        assert_eq!(exit_code(began, SWEPT, Some(0)), reserved, "{began:?}");
        assert_eq!(exit_code(began, SWEPT, None), reserved, "{began:?}");
    }
}

#[test]
fn an_escapee_makes_the_run_exit_escapee_whatever_the_agents_code() {
    let escapee = crate::windows::runner::RunExit::Escapee.code();
    let end = EndSweep::new(Record::default());
    assert!(!end.escaped());
    assert_eq!(end.outcome(0), 0);
    assert_eq!(end.outcome(3), 3);
    // The inner reported one on ctl (a tick's or the end's sweep): the agent's own code, even 0,
    // and a cancel's, give way.
    end.mark_escapee();
    assert!(end.escaped());
    for code in [0, 3, HOST_ENDED, SWEPT] {
        assert_eq!(end.outcome(code), escapee, "{code:#x}");
    }
}

#[test]
fn the_reserved_pool_stop_exit_is_no_clean_end() {
    // No sweeper exits `PoolStop` any more; one that did would be a failure like any other.
    let pool_stop = crate::windows::runner::RunExit::PoolStop.code();
    assert_eq!(Swept::from_exit(Some(pool_stop)), Swept::Unclean);
}

#[test]
fn the_end_sweepers_escapee_exit_is_the_runs_outcome() {
    let escapee = crate::windows::runner::RunExit::Escapee.code();
    let sweep = crate::windows::runner::RunExit::Sweep.code();
    assert_eq!(Swept::from_exit(Some(0)), Swept::Clean);
    assert_eq!(Swept::from_exit(Some(escapee)), Swept::Escapee);
    assert_eq!(Swept::from_exit(Some(sweep)), Swept::Unclean);
    assert_eq!(Swept::from_exit(None), Swept::Unclean);
    let end = EndSweep::new(Record {
        swept: Some(Swept::Escapee),
        ..Record::default()
    });
    assert!(end.trigger(EndSignal::InnerExited));
    assert_eq!(end.wait(), Swept::Escapee);
    assert!(end.escaped());
    assert_eq!(end.outcome(0), escapee);
    for swept in [Swept::Clean, Swept::Unclean] {
        let end = EndSweep::new(Record {
            swept: Some(swept),
            ..Record::default()
        });
        assert_eq!(end.wait(), swept);
        assert_eq!(end.outcome(7), 7, "{swept:?}");
    }
}

#[test]
fn only_a_clean_end_removes_the_unclean_session_marker() {
    let ended = |first: Option<EndSignal>, swept: Swept| {
        let end = EndSweep::new(Record {
            swept: Some(swept),
            ..Record::default()
        });
        if let Some(first) = first {
            end.trigger(first);
        }
        let swept = end.wait();
        (end, swept)
    };
    // The inner exited, reported the agent's exit, and the sweeper exited 0: clean.
    let (end, swept) = ended(Some(EndSignal::InnerExited), Swept::Clean);
    assert!(end.clean(swept, true));
    // ... but not when the inner never reported, or a sweep found an escapee.
    assert!(!end.clean(swept, false));
    end.mark_escapee();
    assert!(!end.clean(swept, true));
    // A cancel, a failure, or no end signal at all (the outer never got that far): left.
    for first in [Some(EndSignal::Stdin), Some(EndSignal::Failed), None] {
        let (end, swept) = ended(first, Swept::Clean);
        assert!(!end.clean(swept, true), "{first:?}");
    }
    // A sweeper that failed, ran past the wait or never started (the job didn't empty), or
    // found an escapee: left.
    for swept in [Swept::Unclean, Swept::Escapee] {
        let (end, swept) = ended(Some(EndSignal::InnerExited), swept);
        assert!(!end.clean(swept, true), "{swept:?}");
    }
    let end = EndSweep::new(Record {
        fail: Some("empty job"),
        ..Record::default()
    });
    end.trigger(EndSignal::InnerExited);
    assert!(!end.clean(end.wait(), true));
}
