//! The session's end through a recording fake: the job emptied first, then the sweep task
//! requested once a session on every end path, even after a job that didn't empty; a failed
//! request logged with the exit code unchanged; the wait for the task's result, clean only at 0;
//! the host's stdin end set apart from the agent's; and the exit code on a cancel.

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
    /// The sweep task's results, read in turn; 0 once they run out.
    results: Mutex<VecDeque<Result<Option<u32>, String>>>,
    /// The task never ends: every read says it is still running.
    running: bool,
    /// How many result reads and pauses were made.
    reads: Mutex<usize>,
    pauses: Mutex<Vec<Duration>>,
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

    /// A fake whose sweep task gives `results` in turn.
    fn answering(results: &[Result<Option<u32>, &str>]) -> Record {
        let results = results
            .iter()
            .map(|result| (*result).map_err(str::to_owned))
            .collect();
        Record {
            results: Mutex::new(results),
            ..Record::default()
        }
    }
}

impl EndSteps for Record {
    type Run = u32;
    fn empty_job(&self) -> Result<(), String> {
        self.step("empty job")
    }
    fn request_sweep(&self) -> Result<u32, String> {
        if let Some(begun) = self.slow.lock().unwrap().take() {
            begun.send(()).unwrap();
            // A request that takes 100 ms: nothing is ever sent on `idle`.
            let (_kept, idle) = mpsc::channel::<()>();
            assert!(idle.recv_timeout(Duration::from_millis(100)).is_err());
        }
        self.step("request sweep").map(|()| 9)
    }
    fn sweep_result(&self, run: &u32) -> Result<Option<u32>, String> {
        assert_eq!(*run, 9);
        *self.reads.lock().unwrap() += 1;
        if self.running {
            return Ok(None);
        }
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(Some(0)))
    }
    fn pause(&self, pause: Duration) {
        self.pauses.lock().unwrap().push(pause);
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
}

const ENDED: [&str; 2] = ["empty job", "request sweep"];
const NOT_EMPTY: [&str; 3] = [
    "empty job",
    "log clauseo-sandbox-run: warning: the job didn't empty (empty job failed (5)); the sandbox \
     sweep task is requested all the same",
    "request sweep",
];
const NOT_REQUESTED: [&str; 3] = [
    "empty job",
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
    assert_eq!(end_session(&record), (true, Some(9)));
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
    assert_eq!(end_session(&record), (false, Some(9)));
    assert_eq!(record.done(), NOT_EMPTY);
    for path in [EndSignal::InnerExited, EndSignal::Failed] {
        let end = SessionEnd::new(not_empty());
        assert!(end.trigger(path), "{path:?}");
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
        assert_eq!(end.began(), Some(path));
        // A later path doesn't end it again.
        assert!(!end.trigger(EndSignal::Stdin));
        assert_eq!(end.steps().done(), NOT_EMPTY, "{path:?}");
        // The sweep ran clean, but the job didn't empty: no clean end.
        assert_eq!(end.wait(), Swept::Clean);
        assert!(!end.clean(Swept::Clean, true), "{path:?}");
    }
    // The host's stdin end: the agent's stdin closes, its grace runs out, the request all the same.
    let end = SessionEnd::new(not_empty());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps().log("agent stdin closed");
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
    // Nothing to wait for: unclean, said once, and the outcome is the agent's.
    assert_eq!(end.wait(), Swept::Unclean);
    assert_eq!(end.wait(), Swept::Unclean);
    let mut want = lines(&NOT_REQUESTED);
    want.push("log clauseo-sandbox-run: warning: no sandbox sweep task was requested".to_owned());
    assert_eq!(end.steps().done(), want);
    assert_eq!(*end.steps().reads.lock().unwrap(), 0);
    assert!(!end.clean(end.wait(), true));
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
    want.extend(lines(&ENDED));
    want.push("log drain".to_owned());
    assert_eq!(end.steps().done(), want);
    // Its end began at the inner's exit: no cancel, and a clean end once the sweep ended 0.
    assert_eq!(end.began(), Some(EndSignal::InnerExited));
    assert!(end.clean(end.wait(), true));
}

#[test]
fn stdin_ending_with_the_agent_not_finishing_ends_the_job_then_requests_the_sweep() {
    let end = SessionEnd::new(Record::default());
    relay_stdin(&b"request\n"[..], io::sink(), &end, SHORT, |_| {
        end.steps().log("agent stdin closed");
    });
    // The grace ran out: the job ended and emptied, then the request, all on this path.
    let mut want = lines(&["log agent stdin closed"]);
    want.extend(lines(&ENDED));
    assert_eq!(end.steps().done(), want);
    assert_eq!(end.began(), Some(EndSignal::Stdin));
    // The job's end ended the inner: the host's job-end code, no second request, no clean end.
    let code = inner_exited(&end, Some(SWEPT), None, || end.steps().log("drain"));
    assert_eq!(code, HOST_ENDED);
    want.extend(lines(&["log drain", "log drain"]));
    assert_eq!(end.steps().done(), want);
    assert!(!end.clean(end.wait(), false));
}

#[test]
fn the_grace_and_the_jobs_end_leave_the_request_half_a_second_of_the_hosts_two() {
    // What the host gives the outer after it closed its stdin, before it ends the outer's job:
    // `STDIN_GRACE` in `crates/process/src/handle.rs`.
    const HOST_BUDGET: Duration = Duration::from_secs(2);
    assert_eq!(GRACE, Duration::from_secs(1));
    assert_eq!(EMPTY_WAIT, Duration::from_millis(400));
    // A full grace, a full wait for the job to empty, and 500 ms for the request's COM calls
    // and the last poll: all inside the 2 s.
    let request = Duration::from_millis(500);
    assert!(GRACE + EMPTY_WAIT + request <= HOST_BUDGET);
    // The outer's own wait for the result, past the host's budget, is for a run with no cancel.
    assert_eq!(SWEEP_WAIT, Duration::from_secs(20));
    assert_eq!(SWEEP_POLL, Duration::from_millis(100));
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
    let code = inner_exited(&end, Some(7), Some(7), || end.steps().log("drain"));
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
                end.steps().log("agent stdin closed");
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
    // A sweep task that ended 0 changes nothing: still the escapee, and no clean end.
    assert!(end.trigger(EndSignal::InnerExited));
    assert_eq!(end.wait(), Swept::Clean);
    assert!(!end.clean(Swept::Clean, true));
    assert_eq!(end.outcome(0), escapee);
}

#[test]
fn the_wait_reads_the_result_every_100_ms_for_up_to_20_s_and_only_0_is_clean() {
    let (none, zero) = (Ok(None), Ok(Some(0)));
    // Queued, running, then 0: two pauses of 100 ms, no line.
    let record = Record::answering(&[none, none, zero]);
    assert_eq!(wait_swept(&record, Some(&9)), Swept::Clean);
    assert_eq!(*record.pauses.lock().unwrap(), [SWEEP_POLL; 2]);
    assert_eq!(record.done(), Vec::<String>::new());
    // Any other result, in hex: a task it couldn't read or delete, and the reserved PoolStop.
    for code in [1, 0x8007_0005, RunExit::PoolStop.code()] {
        let record = Record::answering(&[none, Ok(Some(code))]);
        assert_eq!(wait_swept(&record, Some(&9)), Swept::Unclean, "{code:#x}");
        let line =
            format!("log clauseo-sandbox-run: warning: the sandbox sweep task ended {code:#010X}");
        assert_eq!(record.done(), [line.replace("0X", "0x")], "{code:#x}");
    }
    // Still running at the deadline: 200 pauses, 201 reads, and no more.
    let record = Record {
        running: true,
        ..Record::default()
    };
    assert_eq!(wait_swept(&record, Some(&9)), Swept::Unclean);
    assert_eq!(record.pauses.lock().unwrap().len(), 200);
    assert_eq!(*record.reads.lock().unwrap(), 201);
    assert_eq!(
        record.done(),
        ["log clauseo-sandbox-run: warning: the sandbox sweep task was still running after 20 s"]
    );
    // A read that failed.
    let record = Record::answering(&[Err("IRegisteredTask::GetInstances failed (0x80070005)")]);
    assert_eq!(wait_swept(&record, Some(&9)), Swept::Unclean);
    assert_eq!(
        record.done(),
        [
            "log clauseo-sandbox-run: warning: the sandbox sweep task's result can't be read \
             (IRegisteredTask::GetInstances failed (0x80070005))"
        ]
    );
}

#[test]
fn only_a_clean_end_removes_the_unclean_session_marker() {
    let ended = |first: Option<EndSignal>, record: Record| {
        let end = SessionEnd::new(record);
        if let Some(first) = first {
            end.trigger(first);
        }
        let swept = end.wait();
        (end, swept)
    };
    let inner = Some(EndSignal::InnerExited);
    // The inner exited, reported the agent's exit, the job emptied, and the sweep ended 0: clean.
    let (end, swept) = ended(inner, Record::default());
    assert_eq!(swept, Swept::Clean);
    assert!(end.clean(swept, true));
    // ... but not when the inner never reported, or a sweep found an escapee.
    assert!(!end.clean(swept, false));
    end.mark_escapee();
    assert!(!end.clean(swept, true));
    // A cancel, a failure, or no end signal at all (the outer never got that far): left.
    for first in [Some(EndSignal::Stdin), Some(EndSignal::Failed), None] {
        let (end, swept) = ended(first, Record::default());
        assert!(!end.clean(swept, true), "{first:?}");
    }
    // A sweep task that ended non-zero, ran past the wait, or whose result couldn't be read; a
    // request that failed; a job that didn't empty: left.
    let unclean = [
        Record::answering(&[Ok(Some(1))]),
        Record {
            running: true,
            ..Record::default()
        },
        Record::answering(&[Err("read failed")]),
        Record {
            fail: Some("request sweep"),
            ..Record::default()
        },
        Record {
            fail: Some("empty job"),
            ..Record::default()
        },
    ];
    for (at, record) in unclean.into_iter().enumerate() {
        let (end, swept) = ended(inner, record);
        assert!(!end.clean(swept, true), "case {at}");
    }
}
