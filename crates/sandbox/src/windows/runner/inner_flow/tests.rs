//! The inner runner's order through a recording fake: the first-act sweep and cleanup before the
//! launch, the fail-closed sweep, the sweeps given no job handle, a job list that can't be read,
//! the tick, and the escapee reports at each point.

use super::*;

/// Records each call; fails the ones it is told to.
#[derive(Default)]
struct Record {
    done: Vec<String>,
    /// Sweeps at these points fail.
    fail_sweep: Vec<Point>,
    /// Sweeps at these points can't read their job's pid list.
    job_list_fails: Vec<Point>,
    /// Sweeps at these points find a process they can't end.
    escape_sweep: Vec<Point>,
    /// Reports told, and whether `ctl` was open when they were.
    told: Vec<(InnerReport, bool)>,
    /// This step fails: "handshake", "connect", "read", "wer", "start", "started".
    fail: Option<&'static str>,
    /// How many waits pass before the agent exits.
    waits_left: usize,
    /// Lines logged, and whether `ctl` was open when they were.
    lines: Vec<(String, bool)>,
    open: bool,
}

const ERROR: SweepError = SweepError::StillRunning;

/// A sweep's failure naming a process it couldn't end: the first act's first failure, or a later
/// sweep's left processes, one of them unkillable.
fn escapee(point: Point) -> SweepError {
    if point == Point::Start {
        return SweepError::Open {
            pid: 12,
            image: "hidden.exe".to_owned(),
            code: "5".to_owned(),
        };
    }
    SweepError::Left {
        count: 2,
        unkillable: 1,
        images: vec!["hidden.exe".to_owned()],
    }
}

fn escapee_report(point: &str) -> InnerReport {
    InnerReport::Escapee {
        point: point.to_owned(),
        count: 1,
        images: vec!["hidden.exe".to_owned()],
    }
}

impl Inner for Record {
    type Agent = u32;
    fn sweep(&mut self, point: Point) -> Result<(), SweepError> {
        self.done.push(format!("sweep {point}"));
        if self.fail_sweep.contains(&point) {
            return Err(ERROR);
        }
        if self.job_list_fails.contains(&point) {
            return Err(SweepError::JobList("6".to_owned()));
        }
        if self.escape_sweep.contains(&point) {
            return Err(escapee(point));
        }
        Ok(())
    }
    fn tell(&mut self, report: InnerReport) {
        self.told.push((report, self.open));
    }
    fn clean(&mut self) {
        self.done.push("clean".to_owned());
        self.log("clauseo-sandbox-run: cleanup (before launch): removed 1 Run and RunOnce values");
    }
    fn quiet_wer(&mut self) -> Result<Vec<String>, Failure> {
        self.done.push("quiet wer".to_owned());
        if self.fail == Some("wer") {
            return Err((RunExit::PoolProfile, "WER on".to_owned()));
        }
        if self.fail == Some("wer policy") {
            return Ok(vec![
                "clauseo-sandbox-run: a machine policy (HKLM) keeps WER on".to_owned(),
            ]);
        }
        Ok(Vec::new())
    }
    fn log(&mut self, line: &str) {
        self.lines.push((line.to_owned(), self.open));
    }
    fn handshake(&mut self) -> Result<(), RunExit> {
        self.done.push("handshake".to_owned());
        if self.fail == Some("handshake") {
            return Err(RunExit::Pipe);
        }
        Ok(())
    }
    fn connect(&mut self) -> Result<(), RunExit> {
        self.done.push("connect".to_owned());
        if self.fail == Some("connect") {
            return Err(RunExit::Pipe);
        }
        self.open = true;
        Ok(())
    }
    fn read_launch(&mut self) -> Result<(), Failure> {
        self.done.push("read launch".to_owned());
        if self.fail == Some("read") {
            return Err((RunExit::Pipe, "ctl read failed (109)".to_owned()));
        }
        Ok(())
    }
    fn start(&mut self) -> Result<u32, Failure> {
        self.done.push("start agent".to_owned());
        if self.fail == Some("start") {
            return Err((RunExit::Spawn, "CreateProcessAsUserW failed (5)".to_owned()));
        }
        Ok(77)
    }
    fn pid(&self, agent: &u32) -> u32 {
        *agent
    }
    fn wait(&mut self, _: &u32, interval: Duration) -> Option<u32> {
        self.done.push(format!("wait {}s", interval.as_secs()));
        if self.waits_left == 0 {
            return Some(3);
        }
        self.waits_left -= 1;
        None
    }
    fn kill(&mut self, agent: &u32, code: u32) {
        self.done.push(format!("kill {agent} {code:#x}"));
    }
    fn report(&mut self, report: &InnerReport) -> bool {
        self.done.push(format!("report {report:?}"));
        !(self.fail == Some("started") && matches!(report, InnerReport::Started { .. }))
    }
}

#[test]
fn the_first_act_sweeps_and_cleans_before_the_launch_and_the_agent() {
    let mut record = Record {
        waits_left: 2,
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    assert_eq!(
        record.done,
        [
            "handshake",
            "sweep before launch",
            "clean",
            "connect",
            "read launch",
            "quiet wer",
            "start agent",
            "report Started { pid: 77 }",
            "wait 30s",
            "sweep mid-session",
            "wait 30s",
            "sweep mid-session",
            "wait 30s",
            "sweep session end",
            "report Exited { code: 3 }",
        ]
    );
    // The cleanup's line came before ctl opened: the real calls hold it until `connect`.
    assert_eq!(record.lines.len(), 1);
    assert!(!record.lines[0].1);
}

/// The handshake (ready, then the outer's go after its full check) is the first thing the inner
/// does once resumed; one that fails (no go: the outer's check failed, or `ctl` broke) ends it
/// with nothing else done: no sweep, no launch, no agent.
#[test]
fn nothing_runs_before_the_outers_go() {
    let mut record = Record {
        fail: Some("handshake"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Pipe.code());
    assert_eq!(record.done, ["handshake"]);
    assert!(record.told.is_empty() && record.lines.is_empty());
}

#[test]
fn a_failed_first_act_sweep_exits_sweep_and_starts_no_agent() {
    let mut record = Record {
        fail_sweep: vec![Point::Start],
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Sweep.code());
    let what = "sweep (before launch): the sandbox user still started processes after 8 rounds";
    assert_eq!(
        record.done,
        [
            "handshake".to_owned(),
            "sweep before launch".to_owned(),
            "connect".to_owned(),
            format!(
                "report Failed {{ exit: {}, what: {what:?} }}",
                RunExit::Sweep.code()
            ),
        ]
    );
    // With ctl failing too, the exit code still says the sweep.
    let mut record = Record {
        fail_sweep: vec![Point::Start],
        fail: Some("connect"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Sweep.code());
    assert!(!record.done.iter().any(|step| step == "start agent"));
}

#[test]
fn the_tick_retries_a_failure_and_stops_when_the_agent_exits() {
    let mut record = Record {
        waits_left: 3,
        fail_sweep: vec![Point::Tick, Point::End],
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    let ticks = record
        .done
        .iter()
        .filter(|step| step.starts_with("sweep mid-session"))
        .count();
    let waits = record
        .done
        .iter()
        .filter(|step| *step == "wait 30s")
        .count();
    assert_eq!((ticks, waits), (3, 4));
    assert_eq!(TICK, Duration::from_secs(30));
    // Each failed tick and the failed end sweep are logged on ctl; the agent's exit still ends it.
    let logged: Vec<&str> = record.lines.iter().map(|(line, _)| line.as_str()).collect();
    assert_eq!(logged.len(), 5, "{logged:?}");
    assert!(logged[1].starts_with("clauseo-sandbox-run: sweep (mid-session): "));
    assert!(logged[4].starts_with("clauseo-sandbox-run: sweep (session end): "));
    // The cleanup's line came before ctl opened (the real calls hold it until then); the rest
    // after.
    assert!(!record.lines[0].1);
    assert!(record.lines.iter().skip(1).all(|(_, open)| *open));
    assert_eq!(record.done.last().unwrap(), "report Exited { code: 3 }");
    let end = record
        .done
        .iter()
        .position(|step| step.starts_with("sweep session end"))
        .unwrap();
    assert_eq!(record.done[end - 1], "wait 30s");
}

#[test]
fn a_failure_after_the_first_act_is_reported_and_starts_no_agent() {
    for (fail, exit) in [("read", RunExit::Pipe), ("start", RunExit::Spawn)] {
        let mut record = Record {
            fail: Some(fail),
            ..Record::default()
        };
        assert_eq!(run(&mut record), exit.code(), "{fail}");
        assert!(record.done.last().unwrap().starts_with("report Failed"));
        assert!(!record.done.iter().any(|step| step.starts_with("wait")));
    }
    // An unsent start report ends the agent; its exit is still waited for and swept after.
    let mut record = Record {
        fail: Some("started"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    let kill = format!("kill 77 {:#x}", RunExit::Pipe.code());
    assert!(record.done.contains(&kill), "{:?}", record.done);
}

#[test]
fn an_escapee_at_the_first_act_is_reported_before_ctl_opens_and_exits_escapee() {
    let mut record = Record {
        escape_sweep: vec![Point::Start],
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Escapee.code());
    // Told before ctl opened: the real calls hold it and send it first once `connect` opens it.
    assert_eq!(record.told, [(escapee_report("before launch"), false)]);
    let what = "sweep (before launch): the sandbox user's process 12 (hidden.exe) can't be \
                opened (5)";
    assert_eq!(
        record.done,
        [
            "handshake".to_owned(),
            "sweep before launch".to_owned(),
            "connect".to_owned(),
            format!(
                "report Failed {{ exit: {}, what: {what:?} }}",
                RunExit::Escapee.code()
            ),
        ]
    );
    // With ctl failing too, the exit code still says it.
    let mut record = Record {
        escape_sweep: vec![Point::Start],
        fail: Some("connect"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Escapee.code());
    // Any other first-act failure stays Sweep, and tells nothing.
    let mut record = Record {
        fail_sweep: vec![Point::Start],
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Sweep.code());
    assert_eq!(record.told, Vec::new());
}

#[test]
fn an_escapee_at_a_tick_or_the_end_is_reported_on_ctl_and_the_run_goes_on() {
    let mut record = Record {
        waits_left: 1,
        escape_sweep: vec![Point::Tick, Point::End],
        ..Record::default()
    };
    // The agent's own code: the outer makes the run's outcome Escapee.
    assert_eq!(run(&mut record), 3);
    assert_eq!(
        record.told,
        [
            (escapee_report("mid-session"), true),
            (escapee_report("session end"), true),
        ]
    );
    // Each told after its sweep's log line and before the exit report.
    let logged: Vec<&str> = record.lines.iter().map(|(line, _)| line.as_str()).collect();
    assert!(logged[1].starts_with("clauseo-sandbox-run: sweep (mid-session): 2 of"));
    assert_eq!(record.done.last().unwrap(), "report Exited { code: 3 }");
    // A tick's plain failure tells nothing.
    let mut record = Record {
        waits_left: 1,
        fail_sweep: vec![Point::Tick],
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    assert_eq!(record.told, Vec::new());
}

#[test]
fn a_job_list_that_cant_be_read_fails_the_first_act_closed_and_a_tick_tries_again() {
    // The first act: Sweep, no agent, and no escapee told.
    let mut record = Record {
        job_list_fails: vec![Point::Start],
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::Sweep.code());
    let what = "sweep (before launch): the job's process list failed (6)";
    assert_eq!(
        record.done.last().unwrap(),
        &format!(
            "report Failed {{ exit: {}, what: {what:?} }}",
            RunExit::Sweep.code()
        )
    );
    assert!(!record.done.iter().any(|step| step == "start agent"));
    assert_eq!(record.told, Vec::new());
    // A tick: logged, tried again at the next, and the run goes on.
    let mut record = Record {
        waits_left: 2,
        job_list_fails: vec![Point::Tick],
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    let failed: Vec<&str> = record
        .lines
        .iter()
        .map(|(line, _)| line.as_str())
        .filter(|line| line.contains("the job's process list failed (6)"))
        .collect();
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert_eq!(record.done.last().unwrap(), "report Exited { code: 3 }");
}

#[test]
fn wer_left_on_refuses_the_launch_and_starts_no_agent() {
    let mut record = Record {
        fail: Some("wer"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), RunExit::PoolProfile.code());
    let failed = format!(
        "report Failed {{ exit: {}, what: \"WER on\" }}",
        RunExit::PoolProfile.code()
    );
    assert_eq!(
        record.done,
        [
            "handshake",
            "sweep before launch",
            "clean",
            "connect",
            "read launch",
            "quiet wer",
            failed.as_str(),
        ]
    );
}

#[test]
fn a_wer_policy_keeping_wer_on_is_logged_and_the_agent_starts() {
    let mut record = Record {
        fail: Some("wer policy"),
        ..Record::default()
    };
    assert_eq!(run(&mut record), 3);
    assert!(record.done.contains(&"start agent".to_owned()));
    assert!(
        record
            .lines
            .iter()
            .any(|(line, open)| *open && line.contains("keeps WER on"))
    );
}
