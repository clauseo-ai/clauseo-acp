//! The session's end through a recording fake: the job emptied first, then `SweepNext` read and
//! the sweep task requested once a session on every end path, even after a job that didn't
//! empty; a failed request logged with the exit code unchanged; the marker settled with no wait
//! for any run (awaiting a sweep, or a stop alone after an escapee); the host's stdin end set apart
//! from the agent's; and the exit code on a cancel.

use std::collections::VecDeque;
use std::io;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use super::stdin_tests::Fed;
use super::*;
use crate::windows::runner::sweep::SWEPT;

/// Records each step; fails the one named `fail`.
#[derive(Default)]
struct Record {
    done: Mutex<Vec<String>>,
    fail: Option<&'static str>,
    /// Told when the request begins, which then takes a while.
    slow: Mutex<Option<mpsc::Sender<()>>>,
    /// The markers written, in order.
    settled: Mutex<Vec<MarkerState>>,
    /// The clock's readings, in turn; 0 once they run out.
    readings: Mutex<VecDeque<Duration>>,
}

/// The fake's `SweepNext` and `StopNext`.
const SWEEP_AT: u64 = 12;
const STOP_AT: u64 = 4;

impl Record {
    fn step(&self, name: &str) -> Result<(), Detail> {
        self.done.lock().unwrap().push(name.to_owned());
        if self.fail == Some(name) {
            return Err(Detail::new(|| format!("{name} failed (5)")));
        }
        Ok(())
    }

    fn done(&self) -> Vec<String> {
        self.done.lock().unwrap().clone()
    }

    fn settled(&self) -> Vec<MarkerState> {
        self.settled.lock().unwrap().clone()
    }
}

impl EndSteps for Record {
    fn empty_job(&self) -> Result<(), Detail> {
        self.step("empty job")
    }
    fn next_number(&self, name: &'static str) -> Result<u64, Detail> {
        self.step(&format!("read {name}"))?;
        Ok(if name == SWEEP_NEXT {
            SWEEP_AT
        } else {
            STOP_AT
        })
    }
    fn request_sweep(&self) -> Result<(), Detail> {
        if let Some(begun) = self.slow.lock().unwrap().take() {
            begun.send(()).unwrap();
            // A request that takes 100 ms: nothing is ever sent on `idle`.
            let (_kept, idle) = mpsc::channel::<()>();
            assert!(idle.recv_timeout(Duration::from_millis(100)).is_err());
        }
        self.step("request sweep")
    }
    fn request_stop(&self) -> bool {
        self.step("request stop").is_ok()
    }
    fn settle(&self, state: &MarkerState) -> Result<(), Detail> {
        self.step("settle")?;
        self.settled.lock().unwrap().push(*state);
        Ok(())
    }
    fn log(&self, _kind: RunnerKind, detail: Detail) {
        self.done
            .lock()
            .unwrap()
            .push(format!("log {}", detail.text()));
    }
    fn now(&self) -> Duration {
        self.readings
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
    }
}

const ENDED: [&str; 3] = ["empty job", "read SweepNext", "request sweep"];
const NOT_EMPTY: [&str; 4] = [
    "empty job",
    "log clauseo-sandbox-run: warning: the job didn't empty (empty job failed (5)); the sandbox \
     sweep task is requested all the same",
    "read SweepNext",
    "request sweep",
];
const NOT_REQUESTED: [&str; 4] = [
    "empty job",
    "read SweepNext",
    "request sweep",
    "log clauseo-sandbox-run: warning: the sandbox sweep task couldn't be requested (request \
     sweep failed (5))",
];

/// A grace the tests can wait out.
const SHORT: Duration = Duration::from_millis(50);

/// `want` as owned lines.
fn lines(want: &[&str]) -> Vec<String> {
    want.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn the_job_is_emptied_then_the_sweep_task_is_requested() {
    let record = Record::default();
    assert_eq!(end_session(&record), (true, Some(SWEEP_AT)));
    // The job is empty before the request; no process of the pool user's is started.
    assert_eq!(record.done(), ENDED);
}

#[test]
fn a_job_that_doesnt_empty_still_requests_the_sweep_on_every_path() {
    let not_empty = || Record {
        fail: Some("empty job"),
        ..Record::default()
    };
    let record = not_empty();
    assert_eq!(end_session(&record), (false, Some(SWEEP_AT)));
    assert_eq!(record.done(), NOT_EMPTY);
    for path in [EndSignal::InnerExited, EndSignal::Failed] {
        let end = SessionEnd::new(not_empty());
        assert!(end.trigger(path), "{path:?}");
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
        assert_eq!(end.began(), Some(path));
        // A later path doesn't end it again.
        assert!(!end.trigger(EndSignal::Stdin));
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
        // The next inner's first act kills what is left: the marker awaits a sweep alone.
        end.finish();
        let awaiting = MarkerState::AwaitingSweep { sweep: SWEEP_AT };
        assert_eq!(end.steps().settled(), [awaiting], "{path:?}");
    }
    // The host's stdin end: the agent's stdin closes, its grace runs out, the request all the same.
    let end = SessionEnd::new(not_empty());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps()
            .log(RunnerKind::StdinWriterFailed, "agent stdin closed".into());
    });
    let mut want = lines(&["log agent stdin closed"]);
    want.extend(lines(&NOT_EMPTY));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
}

#[test]
fn a_failed_request_is_logged_and_changes_no_exit_code() {
    let failing = || Record {
        fail: Some("request sweep"),
        ..Record::default()
    };
    let end = SessionEnd::new(failing());
    assert_eq!(inner_exited(&end, Some(7), Some(7), || {}), 7);
    assert_eq!(end.steps().done(), NOT_REQUESTED);
    // The marker still awaits a sweep: a later one, which the next launch asks for, clears it.
    end.finish();
    end.finish();
    let awaiting = MarkerState::AwaitingSweep { sweep: SWEEP_AT };
    assert_eq!(end.steps().settled(), [awaiting], "settled once");
    assert_eq!(end.outcome(7), 7);
    // On a cancel and a failure the code is unchanged too.
    let end = SessionEnd::new(failing());
    relay_stdin(&b""[..], io::sink(), &end, SHORT, drop);
    assert_eq!(end.steps().done(), NOT_REQUESTED);
    assert_eq!(inner_exited(&end, Some(SWEPT), None, || {}), HOST_ENDED);
    let end = SessionEnd::new(failing());
    assert!(end.trigger(EndSignal::Failed));
    assert_eq!(end.steps().done(), NOT_REQUESTED);
    assert_eq!(end.outcome(RunExit::Pipe.code()), RunExit::Pipe.code());
}

#[test]
fn a_path_during_another_paths_end_returns_once_it_is_over() {
    let (begun, requesting) = mpsc::channel();
    let end = SessionEnd::new(Record {
        slow: Mutex::new(Some(begun)),
        ..Record::default()
    });
    std::thread::scope(|scope| {
        let stdin = scope.spawn(|| end.trigger(EndSignal::Stdin));
        requesting.recv().unwrap();
        // The inner exited (the job's end ended it) while the stdin path still requests the
        // sweep: this path waits, so the outer never exits with the request half made.
        assert!(!end.trigger(EndSignal::InnerExited));
        assert_eq!(end.steps().done(), ENDED);
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
    // A count that can't be read is an error too: a job that didn't empty.
    let got = wait_empty(|| Err(io::Error::from_raw_os_error(6)), |_| {}, wait, poll);
    assert_eq!(got, Err("the job's process count failed (6)".to_owned()));
    assert!(EMPTY_POLL < EMPTY_WAIT);
}

#[test]
fn each_end_path_requests_the_sweep_once_whichever_comes_first() {
    let paths = [EndSignal::Stdin, EndSignal::InnerExited, EndSignal::Failed];
    for first in paths {
        let end = SessionEnd::new(Record::default());
        assert!(end.trigger(first), "{first:?}");
        for later in paths {
            assert!(!end.trigger(later), "{first:?} then {later:?}");
        }
        assert_eq!(end.steps().done(), ENDED, "{first:?}");
        assert_eq!(end.began(), Some(first));
    }
    assert_eq!(SessionEnd::new(Record::default()).began(), None);
}

#[test]
fn paths_on_two_threads_request_it_once() {
    let end = SessionEnd::new(Record::default());
    let ended: Vec<bool> = std::thread::scope(|scope| {
        let stdin = scope.spawn(|| end.trigger(EndSignal::Stdin));
        let exited = scope.spawn(|| end.trigger(EndSignal::InnerExited));
        vec![stdin.join().unwrap(), exited.join().unwrap()]
    });
    assert_eq!(ended.iter().filter(|s| **s).count(), 1);
    assert_eq!(end.steps().done(), ENDED);
}

#[test]
fn stdin_ending_with_the_agent_finishing_in_its_grace_keeps_its_code_and_output() {
    // The agent's stdin closes first; the agent finishes and the inner exits within the grace.
    // Its output drains, then the job ends and empties, then the sweep task is requested.
    let end = SessionEnd::new(Record::default());
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
                    end.steps()
                        .log(RunnerKind::StdinWriterFailed, "agent stdin closed".into());
                    closed.send(()).unwrap();
                },
            );
        });
        agent_reads_eof.recv().unwrap();
        // The agent wrote its answer and exited 3; the inner reported it and exited.
        let code = inner_exited(&end, Some(3), Some(3), || {
            end.steps()
                .log(RunnerKind::StdinWriterFailed, "drain".into());
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
    want.extend(lines(&ENDED));
    want.push("log drain".to_owned());
    assert_eq!(end.steps().done(), want);
    // Its end began at the inner's exit: no cancel.
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
}

#[test]
fn stdin_ending_with_the_agent_not_finishing_ends_the_job_then_requests_the_sweep() {
    let end = SessionEnd::new(Record::default());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps()
            .log(RunnerKind::StdinWriterFailed, "agent stdin closed".into());
    });
    // The grace ran out: the job ended and emptied, then the request, all on this path.
    let mut want = lines(&["log agent stdin closed"]);
    want.extend(lines(&ENDED));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
    // The job's end ended the inner: the host's job-end code, no second request. A cancel past
    // the grace awaits a sweep too: the next inner's first act kills what is left.
    let code = inner_exited(&end, Some(SWEPT), None, || {
        end.steps()
            .log(RunnerKind::StdinWriterFailed, "drain".into());
    });
    assert_eq!(code, HOST_ENDED);
    want.extend(lines(&["log drain", "log drain"]));
    assert_eq!(end.steps().done(), want);
    end.finish();
    let awaiting = MarkerState::AwaitingSweep { sweep: SWEEP_AT };
    assert_eq!(end.steps().settled(), [awaiting]);
}

#[test]
fn a_cancels_end_fits_in_3_s_well_inside_the_hosts_5() {
    // What the host gives the outer after it closed its stdin, before it ends the outer's job:
    // `SpawnOptions::stdin_grace` on tier B.
    const HOST_GRACE: Duration = Duration::from_secs(5);
    assert_eq!(GRACE, Duration::from_secs(1));
    assert_eq!(EMPTY_WAIT, Duration::from_millis(400));
    assert_eq!(OUTER_END, Duration::from_secs(3));
    // A full grace, a full wait for the job to empty, the escapee snapshot (230 ms at most,
    // measured), and a second for the two record reads, the requests' COM calls and the marker's
    // write: inside the 3 s the cancel tests pin, which leaves the host's 5 s room.
    let snapshot = Duration::from_millis(230);
    let calls = Duration::from_secs(1);
    assert!(GRACE + EMPTY_WAIT + snapshot + calls <= OUTER_END);
    assert!(OUTER_END < HOST_GRACE);
}

#[test]
fn the_grace_counts_from_the_first_stdin_end() {
    let ms = Duration::from_millis;
    let end = SessionEnd::new(Record {
        readings: Mutex::new([ms(100_000), ms(100_030), ms(100_080)].into()),
        ..Record::default()
    });
    // Before the stdin end, all of it; the clock isn't read.
    assert_eq!(end.grace_left(SHORT), SHORT);
    end.stdin_ended();
    // A second end reads no clock and moves nothing.
    end.stdin_ended();
    assert_eq!(end.grace_left(SHORT), ms(20));
    assert_eq!(end.grace_left(SHORT), Duration::ZERO);
}

#[test]
fn the_inners_exit_drains_then_ends_the_job_with_no_grace() {
    let end = SessionEnd::new(Record::default());
    let code = inner_exited(&end, Some(7), Some(7), || {
        end.steps()
            .log(RunnerKind::StdinWriterFailed, "drain".into());
    });
    assert_eq!(code, 7);
    let mut want = lines(&["log drain"]);
    want.extend(lines(&ENDED));
    want.push("log drain".to_owned());
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    // A wait that failed: Spawn, the sweep requested all the same.
    let end = SessionEnd::new(Record::default());
    let spawn = RunExit::Spawn.code();
    assert_eq!(inner_exited(&end, None, None, || {}), spawn);
    assert_eq!(end.steps().done(), lines(&ENDED));
    // The host's stdin ending after that waits no grace and requests nothing more.
    relay_stdin(&b""[..], io::sink(), &end, Duration::from_secs(30), drop);
    assert_eq!(end.steps().done(), lines(&ENDED));
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
fn the_agent_closing_its_stdin_ends_nothing_but_the_hosts_stdin_end_still_counts() {
    let end = SessionEnd::new(Record::default());
    let (feed, fed) = mpsc::channel::<Vec<u8>>();
    let (closed, agent_closed) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let close = |_| {
                end.steps()
                    .log(RunnerKind::StdinWriterFailed, "agent stdin closed".into());
                closed.send(()).unwrap();
            };
            relay_stdin(Fed(fed), Closed, &end, SHORT, close);
        });
        feed.send(b"request\n".to_vec()).unwrap();
        agent_closed.recv().unwrap();
        // The agent refused the write and its stdin closed: nothing else, while the host's stdin
        // is still open and still read.
        feed.send(b"more\n".to_vec()).unwrap();
        assert_eq!(end.steps().done(), lines(&["log agent stdin closed"]));
        assert_eq!(end.began(), None);
        drop(feed);
    });
    // The host's stdin then ended: its grace ran out, and the job's end and the request followed.
    let mut want = lines(&["log agent stdin closed"]);
    want.extend(lines(&ENDED));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
    assert_eq!(copy(&b""[..], io::sink()), CopyEnd::Source);
    assert_eq!(copy(&b"x"[..], Closed), CopyEnd::Destination);
}

#[test]
fn a_cancel_the_jobs_end_ended_exits_as_it_would_have_without_it() {
    let reserved = RunExit::AgentReserved.code();
    let stdin = Some(EndSignal::Stdin);
    // The agent exited on its own first and the inner reported it, then the job's end ended the
    // inner: the agent's code.
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
    let reserved = RunExit::AgentReserved.code();
    for began in [None, Some(EndSignal::InnerExited), Some(EndSignal::Failed)] {
        assert_eq!(exit_code(began, 7, Some(7)), 7, "{began:?}");
        assert_eq!(exit_code(began, SWEPT, Some(0)), reserved, "{began:?}");
        assert_eq!(exit_code(began, SWEPT, None), reserved, "{began:?}");
    }
}

#[test]
fn an_escapee_makes_the_run_exit_escapee_whatever_the_agents_code_or_the_sweep() {
    let escapee = RunExit::Escapee.code();
    let end = SessionEnd::new(Record::default());
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
    // The end changes nothing: still the escapee.
    assert!(end.trigger(EndSignal::InnerExited));
    end.finish();
    assert_eq!(end.outcome(0), escapee);
}

#[test]
fn every_end_without_an_escapee_awaits_the_sweep_next_it_read_and_never_waits() {
    for first in [EndSignal::InnerExited, EndSignal::Stdin, EndSignal::Failed] {
        let end = SessionEnd::new(Record::default());
        end.trigger(first);
        end.finish();
        let mut want = lines(&ENDED);
        want.push("settle".to_owned());
        // Read once the job ended, before the request; no stop is read or asked for.
        assert_eq!(end.steps().done(), want, "{first:?}");
        let awaiting = MarkerState::AwaitingSweep { sweep: SWEEP_AT };
        assert_eq!(end.steps().settled(), [awaiting], "{first:?}");
    }
}

#[test]
fn an_escapee_reads_stop_next_then_requests_the_stop_and_awaits_it_alone() {
    let end = SessionEnd::new(Record::default());
    end.mark_escapee();
    end.trigger(EndSignal::InnerExited);
    end.finish();
    let mut want = lines(&ENDED);
    want.extend(lines(&["read StopNext", "request stop", "settle"]));
    assert_eq!(end.steps().done(), want);
    // No sweep number: the one read while the escapee lived proves nothing.
    let awaiting = MarkerState::AwaitingStop { stop: STOP_AT };
    assert_eq!(end.steps().settled(), [awaiting]);
    // A stop that couldn't be requested is said; the marker still awaits one.
    let end = SessionEnd::new(Record {
        fail: Some("request stop"),
        ..Record::default()
    });
    end.mark_escapee();
    end.trigger(EndSignal::Failed);
    end.finish();
    assert!(end.steps().done().contains(
        &"log clauseo-sandbox-run: warning: the sandbox stop task couldn't be requested".to_owned()
    ));
    assert_eq!(end.steps().settled(), [awaiting]);
}

#[test]
fn a_number_that_cant_be_read_or_no_end_leaves_the_marker_open() {
    for (fail, escaped) in [("read SweepNext", false), ("read StopNext", true)] {
        let end = SessionEnd::new(Record {
            fail: Some(fail),
            ..Record::default()
        });
        if escaped {
            end.mark_escapee();
        }
        end.trigger(EndSignal::InnerExited);
        end.finish();
        assert_eq!(end.steps().settled(), [], "{fail}");
        let said = format!("{} can't be read", fail.trim_start_matches("read "));
        assert!(
            end.steps().done().iter().any(|line| line.contains(&said)),
            "{fail}"
        );
        // The stop is still requested after an escapee, the sweep always.
        assert!(end.steps().done().contains(&"request sweep".to_owned()));
        assert_eq!(
            end.steps().done().contains(&"request stop".to_owned()),
            escaped,
            "{fail}"
        );
    }
    // An escapee's marker needs no SweepNext: it awaits the stop all the same.
    let end = SessionEnd::new(Record {
        fail: Some("read SweepNext"),
        ..Record::default()
    });
    end.mark_escapee();
    end.trigger(EndSignal::InnerExited);
    end.finish();
    let awaiting = MarkerState::AwaitingStop { stop: STOP_AT };
    assert_eq!(end.steps().settled(), [awaiting]);
    // No end began: nothing read, nothing written.
    let end = SessionEnd::new(Record::default());
    end.finish();
    assert_eq!(end.steps().done(), Vec::<String>::new());
    // A marker that can't be written is said, and stays open.
    let end = SessionEnd::new(Record {
        fail: Some("settle"),
        ..Record::default()
    });
    end.trigger(EndSignal::InnerExited);
    end.finish();
    let last = end.steps().done().pop().unwrap();
    assert!(
        last.contains("the session marker couldn't be written"),
        "{last}"
    );
}
