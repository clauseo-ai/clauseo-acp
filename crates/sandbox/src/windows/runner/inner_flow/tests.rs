//! The inner runner's order through a recording fake: the first-act sweep and cleanup before the
//! launch, the fail-closed sweep, the job kept by the later sweeps, and the tick.

use super::*;

/// Records each call; fails the ones it is told to.
#[derive(Default)]
struct Record {
    done: Vec<String>,
    /// Sweeps at these points fail.
    fail_sweep: Vec<Point>,
    /// This step fails: "connect", "read", "start", "started".
    fail: Option<&'static str>,
    /// How many waits pass before the agent exits.
    waits_left: usize,
    /// Lines logged, and whether `ctl` was open when they were.
    lines: Vec<(String, bool)>,
    open: bool,
}

const ERROR: SweepError = SweepError::StillRunning;

impl Inner for Record {
    type Job = u64;
    type Agent = u32;
    fn sweep(&mut self, point: Point, job: Option<&u64>) -> Result<(), SweepError> {
        self.done.push(format!("sweep {point} job {job:?}"));
        if self.fail_sweep.contains(&point) {
            return Err(ERROR);
        }
        Ok(())
    }
    fn clean(&mut self) {
        self.done.push("clean".to_owned());
        self.log("clauseo-sandbox-run: cleanup (before launch): removed 1 BITS jobs");
    }
    fn log(&mut self, line: &str) {
        self.lines.push((line.to_owned(), self.open));
    }
    fn connect(&mut self) -> Result<(), RunExit> {
        self.done.push("connect".to_owned());
        if self.fail == Some("connect") {
            return Err(RunExit::Pipe);
        }
        self.open = true;
        Ok(())
    }
    fn read_launch(&mut self) -> Result<u64, Failure> {
        self.done.push("read launch".to_owned());
        if self.fail == Some("read") {
            return Err((RunExit::Pipe, "ctl read failed (109)".to_owned()));
        }
        Ok(0x1a4)
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
            "sweep before launch job None",
            "clean",
            "connect",
            "read launch",
            "start agent",
            "report Started { pid: 77 }",
            "wait 30s",
            "sweep mid-session job Some(420)",
            "wait 30s",
            "sweep mid-session job Some(420)",
            "wait 30s",
            "sweep session end job Some(420)",
            "report Exited { code: 3 }",
        ]
    );
    // The cleanup's line came before ctl opened: the real calls hold it until `connect`.
    assert_eq!(record.lines.len(), 1);
    assert!(!record.lines[0].1);
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
            "sweep before launch job None".to_owned(),
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
