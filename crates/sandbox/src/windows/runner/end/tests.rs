//! The end sweeper's start through a recording fake: once a session on every end path, never in
//! the job, and the host's stdin end set apart from the agent's.

use std::io;
use std::sync::Mutex;

use super::*;

/// Records each step; fails the one named `fail`.
#[derive(Default)]
struct Record {
    done: Mutex<Vec<String>>,
    fail: Option<&'static str>,
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
    fn log_on(&self) -> Result<u32, String> {
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

const STARTED: [&str; 4] = ["log on", "shut", "resume", "keep 9"];

#[test]
fn the_sweeper_is_logged_on_shut_and_resumed_and_never_put_in_the_job() {
    let record = Record::default();
    start_sweeper(&record);
    // No step puts it in a job: the host ending the outer's job leaves it running.
    assert_eq!(record.done(), STARTED);
}

#[test]
fn a_sweeper_that_fails_to_start_is_logged_and_the_run_goes_on() {
    let record = Record {
        fail: Some("log on"),
        ..Record::default()
    };
    start_sweeper(&record);
    assert_eq!(
        record.done(),
        [
            "log on",
            "log clauseo-sandbox-run: the end sweeper didn't start: log on failed (5)"
        ]
    );
    let record = Record {
        fail: Some("shut"),
        ..Record::default()
    };
    start_sweeper(&record);
    assert_eq!(
        record.done(),
        [
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
    }
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
