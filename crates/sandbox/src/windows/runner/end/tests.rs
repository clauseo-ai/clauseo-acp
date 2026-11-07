//! The end sweeper's start through a recording fake: the job emptied first, no sweeper while it
//! isn't, once a session on every end path, never in the job, and the host's stdin end set apart
//! from the agent's; and the exit code on a cancel.

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
}

const STARTED: [&str; 5] = ["empty job", "log on", "shut", "resume", "keep 9"];
const NOT_EMPTY: [&str; 2] = [
    "empty job",
    "log clauseo-sandbox-run: no end sweeper: empty job failed (5); the next launch's first act \
     sweeps",
];

#[test]
fn the_job_is_emptied_then_the_sweeper_is_logged_on_shut_and_resumed_and_never_put_in_the_job() {
    let record = Record::default();
    assert!(start_sweeper(&record));
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
    assert!(!start_sweeper(&record));
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
    // The host's stdin end: no sweeper, and the agent's stdin still closes.
    let end = EndSweep::new(not_empty());
    relay_stdin(&b"request\n"[..], io::sink(), &end, |_| {
        end.steps().log("agent stdin closed");
    });
    let mut want: Vec<String> = NOT_EMPTY.iter().map(|s| (*s).to_owned()).collect();
    want.push("log agent stdin closed".to_owned());
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
    // The outer's own wait fits the host's 2 s after the stdin end.
    assert!(EMPTY_WAIT <= Duration::from_millis(1500));
    assert!(EMPTY_POLL < EMPTY_WAIT);
}

#[test]
fn a_sweeper_that_fails_to_start_is_logged_and_the_run_goes_on() {
    let record = Record {
        fail: Some("log on"),
        ..Record::default()
    };
    assert!(!start_sweeper(&record));
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
    assert!(!start_sweeper(&record));
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

#[test]
fn the_hosts_stdin_ending_starts_the_sweeper_before_the_agents_stdin_closes() {
    let end = EndSweep::new(Record::default());
    let mut agent = Vec::new();
    relay_stdin(&b"request\n"[..], &mut agent, &end, |_| {
        end.steps().log("agent stdin closed");
    });
    assert_eq!(agent, b"request\n");
    let mut want: Vec<String> = STARTED.iter().map(|s| (*s).to_owned()).collect();
    want.push("log agent stdin closed".to_owned());
    assert_eq!(end.steps().done(), want);
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
    relay_stdin(&b"request\n"[..], Closed, &end, drop);
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
