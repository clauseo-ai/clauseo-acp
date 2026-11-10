//! The watcher's steps over a fake [`Watcher`]: the refusals, the provider-full exit, the
//! lifetime (connect, disconnect, exit), the owner's re-check, a pipe that fails while serving,
//! and that nothing not of the pool reaches the pipe.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use super::super::lifetime::{FIRST_CLIENT_WAIT, LAST_CLIENT_GRACE, Lifetime, MAX_RUN, Verdict};
use super::super::protocol::{Line, RECORDS_PER_SECOND};
use super::super::serve::{OWNER_CHECK, TICK};
use super::super::{
    Delivery, Failed, Pool, TraceError, WatchExit, Watcher, consumer::Access, consumer::Record,
    watch,
};
use super::{LAWYER, POOL_1, pool, sid};

/// What one tick of the fake gives: clients connected, and records the consumer produced.
#[derive(Default, Clone)]
struct Tick {
    clients: usize,
    records: Vec<Record>,
    lost: u64,
    ended: bool,
}

/// A fake machine: the record, the trace and the pipe, scripted tick by tick.
struct Fake {
    system: bool,
    exe: PathBuf,
    pool: Result<Option<Pool>, Failed>,
    /// What the record reads as at each re-check, in turn; then `pool` again.
    rechecks: VecDeque<Result<Option<Pool>, Failed>>,
    pool_reads: usize,
    not_pool: bool,
    /// Every send fails: the pipe can't keep an instance.
    send_fails: bool,
    /// Kernel-File events the callback sees each tick, and all so far.
    events_per_tick: u64,
    events: u64,
    trace: Result<(), TraceError>,
    pipe: Result<(), Failed>,
    ticks: VecDeque<Tick>,
    /// The clients the current tick has.
    clients: usize,
    now: Duration,
    calls: Vec<String>,
    sent: Vec<String>,
    logs: Vec<(bool, String)>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            system: true,
            exe: PathBuf::from(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe"),
            pool: Ok(Some(Pool {
                owner: sid(LAWYER),
                users: pool(),
            })),
            rechecks: VecDeque::new(),
            pool_reads: 0,
            not_pool: false,
            send_fails: false,
            events_per_tick: 0,
            events: 0,
            trace: Ok(()),
            pipe: Ok(()),
            ticks: VecDeque::new(),
            clients: 0,
            now: Duration::ZERO,
            calls: Vec::new(),
            sent: Vec::new(),
            logs: Vec::new(),
        }
    }

    /// `n` ticks with `clients` connected and nothing produced.
    fn idle(mut self, n: usize, clients: usize) -> Fake {
        for _ in 0..n {
            self.ticks.push_back(Tick {
                clients,
                ..Tick::default()
            });
        }
        self
    }

    fn tick(mut self, tick: Tick) -> Fake {
        self.ticks.push_back(tick);
        self
    }

    fn last_log(&self) -> &str {
        self.logs.last().map_or("", |(_, line)| line.as_str())
    }
}

impl Watcher for Fake {
    fn is_system(&mut self) -> Result<bool, Failed> {
        Ok(self.system)
    }
    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        Ok(self.exe.clone())
    }
    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        Ok(PathBuf::from(
            r"c:\program files\clauseo\sandbox\clauseo-sandbox-setup.exe",
        ))
    }
    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        self.pool_reads += 1;
        if self.pool_reads > 1
            && let Some(again) = self.rechecks.pop_front()
        {
            return again;
        }
        self.pool.clone()
    }
    fn check_pool(&mut self, _pool: &Pool) -> Result<(), Failed> {
        if self.not_pool {
            return Err("a SID PoolSids lists failed the pool-user check".to_owned());
        }
        Ok(())
    }
    fn start_trace(&mut self, pool: &[Sid]) -> Result<(), TraceError> {
        self.calls.push(format!("start_trace {}", pool.len()));
        self.trace.clone()
    }
    fn stop_trace(&mut self) {
        self.calls.push("stop_trace".to_owned());
    }
    fn open_pipe(&mut self, owner: &Sid) -> Result<(), Failed> {
        self.calls.push(format!("open_pipe {owner}"));
        self.pipe.clone()
    }
    fn close_pipe(&mut self) {
        self.calls.push("close_pipe".to_owned());
    }
    fn events(&mut self) -> u64 {
        self.events
    }
    fn pause_trace(&mut self) -> Result<(), Failed> {
        self.calls.push(format!("pause_trace {:?}", self.now));
        Ok(())
    }
    fn resume_trace(&mut self) -> Result<(), Failed> {
        self.calls.push(format!("resume_trace {:?}", self.now));
        Ok(())
    }
    fn rate_log(&mut self) -> bool {
        // The breaker's test is the one with events: it logs their rate.
        self.events_per_tick > 0
    }
    fn records(&mut self, wait: Duration) -> Delivery {
        self.now += wait;
        self.events += self.events_per_tick;
        // Past the script, the last tick's clients stay and nothing comes.
        let tick = self.ticks.pop_front().unwrap_or(Tick {
            clients: self.clients,
            ..Tick::default()
        });
        self.clients = tick.clients;
        if tick.ended {
            return Delivery::Ended;
        }
        Delivery::Records(tick.records, tick.lost)
    }
    fn clients(&mut self) -> Result<usize, Failed> {
        Ok(self.clients)
    }
    fn send(&mut self, line: &str) -> Result<(), Failed> {
        if self.send_fails {
            return Err("a later instance couldn't be made".to_owned());
        }
        if self.clients > 0 {
            self.sent.push(line.to_owned());
        }
        Ok(())
    }
    fn now(&mut self) -> Duration {
        self.now
    }
    fn log(&mut self, error: bool, line: &str) {
        self.logs.push((error, line.to_owned()));
    }
}

use crate::windows::sids::Sid;

fn record(who: &str, path: &str) -> Record {
    Record {
        sid: sid(who),
        pid: 1234,
        path: path.to_owned(),
        net: false,
        access: Access::Unknown,
    }
}

/// Ticks in `span`.
fn ticks(span: Duration) -> usize {
    usize::try_from(span.as_millis() / TICK.as_millis()).unwrap()
}

#[test]
fn a_client_gets_the_pools_refusals_and_the_run_ends_after_it_leaves() {
    let mut fake = Fake::new().idle(2, 0).idle(1, 1).tick(Tick {
        clients: 1,
        records: vec![record(POOL_1, r"C:\Users\lawyer\a.txt")],
        ..Tick::default()
    });
    fake = fake.idle(3, 1).idle(ticks(LAST_CLIENT_GRACE) + 5, 0);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert_eq!(fake.sent.len(), 1, "{:?}", fake.sent);
    assert!(matches!(
        Line::parse(fake.sent[0].trim_end()),
        Some(Line::Denied { pid: 1234, .. })
    ));
    assert_eq!(
        fake.calls,
        [
            "start_trace 2",
            &format!("open_pipe {LAWYER}"),
            "close_pipe",
            "stop_trace"
        ]
    );
    // It ended within the grace of the client leaving: 7 ticks served, then the grace.
    let left = TICK * 7;
    assert!(fake.now >= left + LAST_CLIENT_GRACE, "{:?}", fake.now);
    assert!(
        fake.now <= left + LAST_CLIENT_GRACE + TICK,
        "{:?}",
        fake.now
    );
    assert!(
        fake.last_log().contains("the last client left"),
        "{}",
        fake.last_log()
    );
    assert!(!fake.last_log().contains("Users"), "no path in the log");
}

#[test]
fn no_client_within_the_first_wait_ends_the_run() {
    let mut fake = Fake::new();
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert!(fake.now >= FIRST_CLIENT_WAIT && fake.now <= FIRST_CLIENT_WAIT + TICK);
    assert!(fake.last_log().contains("no client connected"));
    assert_eq!(fake.calls.last().map(String::as_str), Some("stop_trace"));
}

#[test]
fn a_client_back_within_the_grace_keeps_the_run() {
    let grace = ticks(LAST_CLIENT_GRACE);
    let mut fake = Fake::new().idle(1, 1).idle(grace - 2, 0).idle(4, 2);
    fake = fake.idle(grace + 2, 0);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert!(fake.now >= TICK * u32::try_from(grace + 3).unwrap() + LAST_CLIENT_GRACE);
    assert!(fake.last_log().contains("3 clients"), "{}", fake.last_log());
}

#[test]
fn the_lifetime_steps_through_its_phases() {
    let s = Duration::from_secs;
    let mut life = Lifetime::new(s(0));
    assert_eq!(life.step(s(29), 0), Verdict::Go);
    assert_eq!(life.step(s(30), 0), Verdict::NoClient);
    let mut life = Lifetime::new(s(0));
    assert_eq!(life.step(s(10), 1), Verdict::Go);
    assert_eq!(life.step(s(20), 0), Verdict::Go);
    assert_eq!(life.step(s(22), 0), Verdict::Go);
    assert_eq!(life.step(s(22), 1), Verdict::Go);
    assert_eq!(life.step(s(40), 0), Verdict::Go);
    assert_eq!(life.step(s(43), 0), Verdict::LastLeft);
    // A client that stays still ends at the run's limit.
    let mut life = Lifetime::new(s(0));
    assert_eq!(life.step(s(1), 1), Verdict::Go);
    assert_eq!(life.step(MAX_RUN, 1), Verdict::TooLong);
}

#[test]
fn a_full_provider_exits_68_and_opens_no_pipe() {
    let mut fake = Fake::new();
    fake.trace = Err(TraceError::ProviderFull(
        "EnableTraceEx2 failed (1450)".to_owned(),
    ));
    let exit = watch(&mut fake);
    assert_eq!(exit, WatchExit::ProviderFull);
    assert_eq!(exit.as_code(), 68);
    assert_eq!(fake.calls, ["start_trace 2"]);
    assert!(fake.logs.last().is_some_and(|(error, _)| *error));
    // Any other trace failure is 73.
    let mut fake = Fake::new();
    fake.trace = Err(TraceError::Other("StartTraceW failed (5)".to_owned()));
    assert_eq!(watch(&mut fake).as_code(), 73);
}

#[test]
fn a_pipe_that_fails_stops_the_trace_and_exits_74() {
    let mut fake = Fake::new();
    fake.pipe = Err("CreateNamedPipeW failed (231)".to_owned());
    assert_eq!(watch(&mut fake), WatchExit::NoPipe);
    assert_eq!(fake.calls.last().map(String::as_str), Some("stop_trace"));
    assert!(fake.calls.contains(&"close_pipe".to_owned()));
}

#[test]
fn a_trace_that_ends_while_serving_exits_75() {
    let mut fake = Fake::new().idle(2, 1).tick(Tick {
        clients: 1,
        ended: true,
        ..Tick::default()
    });
    assert_eq!(watch(&mut fake).as_code(), 75);
    assert_eq!(fake.calls.last().map(String::as_str), Some("stop_trace"));
}

#[test]
fn the_refusals_and_record_are_checked_first() {
    let mut fake = Fake::new();
    fake.system = false;
    assert_eq!(watch(&mut fake).as_code(), 70);
    assert_eq!(fake.calls, Vec::<String>::new());
    let mut fake = Fake::new();
    fake.exe = PathBuf::from(r"C:\Users\lawyer\AppData\clauseo-sandbox-setup.exe");
    assert_eq!(watch(&mut fake).as_code(), 70);
    let mut fake = Fake::new();
    fake.pool = Err("RegGetValueW failed (5)".to_owned());
    assert_eq!(watch(&mut fake).as_code(), 71);
    let mut fake = Fake::new();
    fake.not_pool = true;
    assert_eq!(watch(&mut fake).as_code(), 72);
    assert_eq!(fake.calls, Vec::<String>::new());
    // No record is 71, as a malformed one is: no session.
    let mut fake = Fake::new();
    fake.pool = Ok(None);
    assert_eq!(watch(&mut fake).as_code(), 71);
    assert_eq!(fake.calls, Vec::<String>::new());
    // An empty pool is nothing to watch: 0, no session.
    let mut fake = Fake::new();
    fake.pool = Ok(Some(Pool {
        owner: sid(LAWYER),
        users: Vec::new(),
    }));
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert_eq!(fake.calls, Vec::<String>::new());
}

#[test]
fn a_non_pool_record_never_reaches_the_pipe_writer() {
    // Were the consumer ever to pass one on, the writer refuses it and says so, without its SID.
    let mut fake = Fake::new().idle(1, 1).tick(Tick {
        clients: 1,
        records: vec![record(LAWYER, r"C:\Users\lawyer\private.txt")],
        ..Tick::default()
    });
    fake = fake.idle(ticks(LAST_CLIENT_GRACE) + 2, 0);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert_eq!(fake.sent, Vec::<String>::new());
    let log = fake.last_log();
    assert!(log.contains("1 not of the pool refused"), "{log}");
    assert!(!log.contains(LAWYER) && !log.contains("private"), "{log}");
}

#[test]
fn past_the_rate_the_rest_go_as_one_dropped_line() {
    let n = RECORDS_PER_SECOND as usize + 30;
    let records = vec![record(POOL_1, r"C:\x"); n];
    let mut fake = Fake::new().idle(1, 1).tick(Tick {
        clients: 1,
        records,
        lost: 4,
        ..Tick::default()
    });
    fake = fake.idle(8, 1).idle(ticks(LAST_CLIENT_GRACE) + 2, 0);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    let denied = fake
        .sent
        .iter()
        .filter(|line| matches!(Line::parse(line.trim_end()), Some(Line::Denied { .. })))
        .count();
    assert_eq!(denied, RECORDS_PER_SECOND as usize);
    let dropped: Vec<_> = fake
        .sent
        .iter()
        .filter_map(|line| match Line::parse(line.trim_end()) {
            Some(Line::Dropped { dropped, .. }) => Some(dropped),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, [34]);
}

/// The record as read again with `owner` as its owner.
fn owned_by(owner: &str) -> Pool {
    Pool {
        owner: sid(owner),
        users: pool(),
    }
}

#[test]
fn another_owner_in_the_record_ends_the_run_with_0() {
    let ticks_per_check = ticks(OWNER_CHECK);
    let mut fake = Fake::new().idle(4 * ticks_per_check, 1);
    // The first re-check reads the same owner, an unreadable one is let be, then a new set-up.
    fake.rechecks = VecDeque::from([
        Ok(Some(owned_by(LAWYER))),
        Err("RegGetValueW failed (2)".to_owned()),
        Ok(Some(owned_by(POOL_1))),
    ]);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert!(
        fake.last_log().contains("the record's owner changed"),
        "{}",
        fake.last_log()
    );
    assert_eq!(fake.pool_reads, 4);
    assert!(fake.now <= OWNER_CHECK * 3 + TICK, "{:?}", fake.now);
    assert_eq!(
        fake.calls[fake.calls.len() - 2..],
        ["close_pipe", "stop_trace"]
    );
    // A record gone since is no owner: the run ends too.
    let mut fake = Fake::new().idle(2 * ticks_per_check, 1);
    fake.rechecks = VecDeque::from([Ok(None)]);
    assert_eq!(watch(&mut fake), WatchExit::Done);
    assert!(fake.last_log().contains("the record's owner changed"));
}

#[test]
fn a_pipe_that_fails_to_send_ends_the_run_with_74() {
    let mut fake = Fake::new().idle(1, 1).tick(Tick {
        clients: 1,
        records: vec![record(POOL_1, r"C:\x")],
        ..Tick::default()
    });
    fake.send_fails = true;
    assert_eq!(watch(&mut fake), WatchExit::NoPipe);
    assert_eq!(
        fake.calls[fake.calls.len() - 2..],
        ["close_pipe", "stop_trace"]
    );
}

#[test]
fn a_busy_machine_pauses_the_trace_tells_the_clients_and_resumes() {
    use super::super::breaker::{PAUSE, PAUSE_RATE, WINDOW_SECONDS};
    // Hot from the start: the rate per tick is the breaker's rate a quarter-second.
    let mut fake = Fake::new().idle(ticks(Duration::from_secs(75)), 1);
    fake = fake.idle(ticks(LAST_CLIENT_GRACE) + 2, 0);
    fake.events_per_tick = PAUSE_RATE / 4 + 10;
    assert_eq!(watch(&mut fake), WatchExit::Done);
    let window = Duration::from_secs(u64::from(WINDOW_SECONDS));
    let paused = format!("pause_trace {window:?}");
    let resumed = format!("resume_trace {:?}", window + PAUSE);
    assert!(fake.calls.contains(&paused), "{:?}", fake.calls);
    assert!(fake.calls.contains(&resumed), "{:?}", fake.calls);
    let lines: Vec<_> = fake
        .sent
        .iter()
        .filter_map(|line| Line::parse(line.trim_end()))
        .collect();
    assert!(matches!(
        &lines[0],
        Line::Paused { paused, .. } if paused.secs == 60 && paused.reason == "rate"
    ));
    assert!(matches!(lines[1], Line::Resumed { resumed: true, .. }));
    // The rate log is a number a line, and the end line counts the trips.
    let rates = fake
        .logs
        .iter()
        .filter(|(_, line)| line.starts_with("the sandbox's watcher's event rate: "))
        .count();
    assert!(rates >= 75, "{rates}");
    assert!(
        fake.last_log().contains("paused 1 times"),
        "{}",
        fake.last_log()
    );
}
