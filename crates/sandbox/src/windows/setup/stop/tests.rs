//! `--stop` through a fake machine, on every OS: the refusals stop nothing, only the record's
//! pool users' processes are stopped, pass after pass until one finds none or the time runs out,
//! and the users found are marked for a reset. How each process's user is read is in
//! `tests/users.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;
use crate::windows::setup::record_values::{STOP_COMPLETED, STOP_NEXT, STOP_SEQUENCE};

mod completed;
mod jobs;
mod lock;
mod record;
mod sequence;
mod users;

const OWNER: &str = "S-1-5-21-1-2-3-1001";
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const OWN_PID: u32 = 900;

/// The fake clock's `elapsed` as a `FILETIME`, from an arbitrary start.
fn filetime(elapsed: Duration) -> u64 {
    133_000_000_000_000_000 + u64::try_from(elapsed.as_nanos() / 100).unwrap()
}

fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1-2-3-{rid}")).unwrap()
}

/// How the fake's marking goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marks {
    /// Written.
    Written,
    /// The set-up lock stays held, once the reservation has taken and given it back.
    LockBusy,
    /// The write fails.
    WriteFails,
    /// The marks are written, but `StopCompleted`'s write fails.
    CompletionFails,
    /// The set-up lock stays held from the first try, the reservation's.
    ReserveBusy,
}

/// The fake: the record, the processes each snapshot sees, and everything done, in order.
struct Fake {
    system: bool,
    exe: PathBuf,
    pool: Result<Option<Pool>, Failed>,
    /// The pool-user check refuses.
    not_pool: bool,
    /// Each snapshot, in turn; past the last, the last again.
    snapshots: Vec<Result<Vec<Process>, Failed>>,
    taken: usize,
    /// The fake clock, one second on for each snapshot.
    now: Duration,
    /// The BITS jobs' owners; `Err` when the list can't be read.
    jobs: Result<Vec<Option<Sid>>, Failed>,
    /// Indexes of the jobs whose cancel fails.
    jobs_refuse: Vec<usize>,
    /// Pids whose termination fails.
    refuse: Vec<u32>,
    /// Pids whose first termination fails.
    refuse_once: Vec<u32>,
    /// Pids that never exit once terminated: each termination waits out its wait, on the
    /// fake clock, and fails.
    hang: Vec<u32>,
    /// The wait each termination was given, in order.
    waits: Vec<Duration>,
    /// Why the marks fail, if they do.
    marks: Marks,
    held: bool,
    /// The record's `ResetSids`.
    resets: Vec<Sid>,
    /// The record's `REG_QWORD` values: `StopNext`, `StopCompleted`, `StopSequence`.
    values: BTreeMap<&'static str, u64>,
    /// The set-up lock's tries.
    locks: usize,
    /// The record's reads (`Stopper::pool`).
    pool_reads: usize,
    /// From this read of the record on (0 the first), the pool it gives: `Ok(None)` is the key
    /// gone, which no write can make again.
    pool_from: Option<(usize, Result<Option<Pool>, Failed>)>,
    /// The `Stop*` value whose reads fail.
    read_fails: Option<&'static str>,
    /// The task's end: terminating this pid kills the run there, as the Task Scheduler's end
    /// (or its time limit) kills the process.
    end_at: Option<u32>,
    /// The sweep task's requests, each with how many snapshots were taken by then.
    sweeps: Vec<usize>,
    done: Vec<String>,
    logged: Vec<(bool, String)>,
}

impl Fake {
    fn new(users: &[u32]) -> Fake {
        Fake {
            system: true,
            exe: PathBuf::from(ADMIN_EXE),
            pool: Ok(Some(Pool {
                owner: Sid::parse(OWNER).unwrap(),
                users: users.iter().map(|rid| sid(*rid)).collect(),
            })),
            not_pool: false,
            snapshots: Vec::new(),
            taken: 0,
            now: Duration::ZERO,
            jobs: Ok(Vec::new()),
            jobs_refuse: Vec::new(),
            refuse: Vec::new(),
            refuse_once: Vec::new(),
            hang: Vec::new(),
            waits: Vec::new(),
            marks: Marks::Written,
            held: false,
            resets: Vec::new(),
            values: BTreeMap::new(),
            locks: 0,
            pool_reads: 0,
            pool_from: None,
            read_fails: None,
            end_at: None,
            sweeps: Vec::new(),
            done: Vec::new(),
            logged: Vec::new(),
        }
    }

    /// The pids `done` terminated, in order.
    fn stopped(&self) -> Vec<u32> {
        let pids = self
            .done
            .iter()
            .filter_map(|line| line.strip_prefix("stop "));
        pids.map(|pid| pid.parse().unwrap()).collect()
    }

    /// `StopCompleted` and `StopSequence`, once both are written.
    fn completed(&self) -> Option<(u64, u64)> {
        let at = self.values.get(STOP_COMPLETED)?;
        Some((*at, *self.values.get(STOP_SEQUENCE)?))
    }

    /// An earlier run's `StopCompleted` and `StopSequence`, or neither.
    fn set_completed(&mut self, pair: Option<(u64, u64)>) {
        for name in [STOP_COMPLETED, STOP_SEQUENCE] {
            self.values.remove(name);
        }
        if let Some((at, sequence)) = pair {
            self.values.insert(STOP_COMPLETED, at);
            self.values.insert(STOP_SEQUENCE, sequence);
        }
    }

    /// Whether the record's key is gone, as the last read of the record found it.
    fn gone(&self) -> bool {
        matches!(&self.pool_from, Some((from, Ok(None))) if self.pool_reads > *from)
    }

    /// The snapshots taken.
    fn snapshots_taken(&self) -> usize {
        self.done.iter().filter(|line| *line == "snapshot").count()
    }
}

/// A process the list and the token both say runs as `user`, opened to be stopped, in session
/// 1; with no user, one no source names that can't be opened to be stopped, in session 0 with
/// no image file (a protected system process).
fn seen(pid: u32, user: Option<Sid>) -> Process {
    let can_stop = user.is_some();
    Process {
        pid,
        listed: user.clone(),
        token: user,
        can_stop,
        session: Some(u32::from(can_stop)),
        image: Image::Missing,
        ..Process::default()
    }
}

impl Gate for Fake {
    fn is_system(&mut self) -> Result<bool, Failed> {
        Ok(self.system)
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        Ok(self.exe.clone())
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        Ok(PathBuf::from(ADMIN_EXE))
    }

    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        let at = self.pool_reads;
        self.pool_reads += 1;
        match &self.pool_from {
            Some((from, pool)) if at >= *from => pool.clone(),
            _ => self.pool.clone(),
        }
    }

    fn check_pool(&mut self, _pool: &Pool) -> Result<(), Failed> {
        if self.not_pool {
            return Err("S-1-5-21-1-2-3-500 isn't a pool user".to_owned());
        }
        Ok(())
    }
}

impl Stopper for Fake {
    fn snapshot(&mut self) -> Result<Vec<Process>, Failed> {
        self.done.push("snapshot".to_owned());
        self.now += Duration::from_secs(1);
        let at = self.taken.min(self.snapshots.len().saturating_sub(1));
        self.taken += 1;
        self.snapshots.get(at).cloned().unwrap_or(Ok(Vec::new()))
    }

    fn own_pid(&mut self) -> u32 {
        OWN_PID
    }

    fn suspend(&mut self, pid: u32) -> Result<(), Failed> {
        self.done.push(format!("suspend {pid}"));
        Ok(())
    }

    fn terminate(&mut self, pid: u32, wait: Duration) -> Result<(), Failed> {
        self.waits.push(wait);
        assert_ne!(self.end_at, Some(pid), "the task was ended");
        if self.hang.contains(&pid) {
            self.now += wait;
            return Err("it hadn't exited when the wait ran out".to_owned());
        }
        if let Some(at) = self.refuse_once.iter().position(|once| *once == pid) {
            self.refuse_once.remove(at);
            return Err("access denied".to_owned());
        }
        if self.refuse.contains(&pid) {
            return Err("access denied".to_owned());
        }
        self.done.push(format!("stop {pid}"));
        Ok(())
    }

    fn elapsed(&mut self) -> Duration {
        self.now
    }

    fn pause(&mut self) {
        self.done.push("pause".to_owned());
    }

    fn mark_for_reset(&mut self, users: &[Sid]) -> Result<(), Failed> {
        assert!(self.held, "marked under the set-up lock");
        if self.marks == Marks::WriteFails {
            return Err("RegSetValueExW failed (5)".to_owned());
        }
        self.resets = crate::windows::setup::record_values::resets_with(&self.resets, users);
        self.done.push(format!("mark {}", users.len()));
        Ok(())
    }

    fn request_sweep(&mut self) -> Result<(), Failed> {
        self.sweeps.push(self.snapshots_taken());
        Ok(())
    }

    fn log(&mut self, error: bool, line: &str) {
        self.logged.push((error, line.to_owned()));
    }
}

impl Bits for Fake {
    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed> {
        self.done.push("jobs".to_owned());
        self.jobs.clone()
    }

    fn cancel_job(&mut self, at: usize) -> Result<(), Failed> {
        if self.jobs_refuse.contains(&at) {
            return Err("IBackgroundCopyJob::Cancel failed (0x80070005)".to_owned());
        }
        self.done.push(format!("cancel {at}"));
        Ok(())
    }
}

/// A machine where pool users 1010 and 1011 run, beside the owner, SYSTEM, another local
/// account, a protected process no source could read, this process, and the kernel's two.
fn busy() -> Vec<Process> {
    vec![
        seen(0, None),
        seen(4, Some(Sid::parse("S-1-5-18").unwrap())),
        seen(10, Some(sid(1010))),
        seen(11, Some(Sid::parse(OWNER).unwrap())),
        seen(12, Some(sid(1011))),
        seen(13, Some(Sid::parse("S-1-5-18").unwrap())),
        seen(14, Some(sid(1013))),
        seen(15, None),
        seen(16, Some(sid(1010))),
        seen(OWN_PID, Some(Sid::parse("S-1-5-18").unwrap())),
    ]
}

#[test]
fn only_the_records_pool_users_processes_are_stopped_and_those_users_marked() {
    let mut fake = Fake::new(&[1010, 1011, 1012]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    // 1013 runs as a local account the record doesn't list: left alone, as are the owner,
    // SYSTEM, the unreadable process, this one and the kernel's.
    assert_eq!(fake.stopped(), [10, 12, 16]);
    // Their users marked first, then every one suspended before any is terminated; the second
    // snapshot finds none, and ends.
    let suspend_then_stop = [
        "set StopNext",
        "snapshot",
        "mark 2",
        "suspend 10",
        "suspend 12",
        "suspend 16",
        "stop 10",
        "stop 12",
        "stop 16",
        "snapshot",
        "jobs",
        "set StopCompleted",
        "set StopSequence",
    ];
    assert_eq!(fake.done, suspend_then_stop);
    assert_eq!(fake.resets, [sid(1010), sid(1011)], "1012 wasn't running");
    assert!(!fake.held, "the lock is given back");
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert_eq!(
        line,
        "the sandbox's stop: 3 pool users, 2 found running, 3 processes stopped in 2 passes, \
         0 left, 0 unknown, 0 BITS jobs cancelled, 0 not; the sweep task was requested; 2 \
         users marked for a profile reset (0 retried)"
    );
    // The sweep is asked for once, after the last pass.
    assert_eq!(fake.sweeps, [2]);
    assert!(!line.contains("S-1-5"), "counts only: {line}");
}

#[test]
fn the_next_pass_stops_what_the_last_left_or_what_started_since() {
    let mut fake = Fake::new(&[1010, 1011]);
    // 10 is refused once in the first pass; the second finds it again, with 20, new since; the
    // third finds 21, started by 20 before it ended; the fourth finds none.
    fake.refuse_once = vec![10];
    let second = vec![seen(10, Some(sid(1010))), seen(20, Some(sid(1011)))];
    let third = vec![seen(21, Some(sid(1011)))];
    fake.snapshots = vec![Ok(busy()), Ok(second), Ok(third), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.stopped(), [12, 16, 10, 20, 21]);
    assert_eq!(fake.snapshots_taken(), 4);
    // Only the pass with a refusal pauses before the next.
    assert_eq!(fake.done.iter().filter(|line| *line == "pause").count(), 1);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert!(
        fake.logged[0].1.contains("(1 retried)"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn a_pool_that_never_empties_is_left_at_the_time_limit_but_its_users_are_marked() {
    let mut fake = Fake::new(&[1010, 1011]);
    // 12 refuses every time, and 10 and 16 come back in every snapshot (the last, again).
    fake.refuse = vec![12];
    fake.snapshots = vec![Ok(busy())];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(StopExit::NotStopped.as_code(), 48);
    // One snapshot a second on the fake clock: the 60th is at the limit, and stops nothing.
    let limit = usize::try_from(PASS_LIMIT.as_secs()).unwrap();
    assert_eq!(fake.snapshots_taken(), limit);
    assert_eq!(fake.stopped().len(), 2 * (limit - 1));
    // Marked by the first pass, once.
    let marks: Vec<&String> = fake.done.iter().filter(|l| l.starts_with("mark")).collect();
    assert_eq!(marks, ["mark 2"]);
    assert_eq!(fake.done[2], "mark 2");
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("3 left"), "{line}");
    assert!(line.contains("time ran out"), "{line}");
    // A snapshot that can't be taken is the same exit; what the other pass found is marked.
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![
        Ok(busy()),
        Err("CreateToolhelp32Snapshot failed (5)".to_owned()),
    ];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
}

#[test]
fn each_user_is_marked_when_first_found_before_its_processes_are_stopped() {
    let mut fake = Fake::new(&[1010, 1011]);
    // 1010 runs first; 1011 only shows up in the second pass. 1011 also owns a BITS job, but is
    // marked already, so the BITS step marks no one new.
    let second = vec![seen(10, Some(sid(1010))), seen(20, Some(sid(1011)))];
    fake.snapshots = vec![
        Ok(vec![seen(10, Some(sid(1010)))]),
        Ok(second),
        Ok(Vec::new()),
    ];
    fake.refuse_once = vec![10];
    fake.jobs = Ok(vec![Some(sid(1011))]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    let order = [
        "set StopNext",
        "snapshot",
        "mark 1",
        "suspend 10",
        "pause",
        "snapshot",
        "mark 1",
        "suspend 10",
        "suspend 20",
        "stop 10",
        "stop 20",
        "snapshot",
        "jobs",
        "cancel 0",
        "set StopCompleted",
        "set StopSequence",
    ];
    assert_eq!(fake.done, order);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert!(
        fake.logged[0].1.contains("2 users marked"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn the_waits_past_the_time_limit_are_capped_so_the_run_ends_inside_the_tasks_limit() {
    let mut fake = Fake::new(&[1010]);
    // Forty processes that never exit: each pass would wait 40 s for them.
    let pids: Vec<u32> = (100..140).collect();
    fake.hang.clone_from(&pids);
    let hung = pids.iter().map(|pid| seen(*pid, Some(sid(1010)))).collect();
    fake.snapshots = vec![Ok(hung)];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    // The second pass begins at 42 s, before the limit, and waits until 75 s, then not at all.
    let cap = PASS_LIMIT + LAST_WAITS;
    assert!(fake.now <= cap + Duration::from_secs(1), "{:?}", fake.now);
    assert!(fake.waits.iter().all(|wait| *wait <= EXIT_WAIT));
    let total: Duration = fake.waits.iter().sum();
    // 73 s: the cap, less the two snapshots' seconds.
    assert_eq!(total, Duration::from_secs(73));
    assert_eq!(fake.waits.last(), Some(&Duration::ZERO));
    // Its user was marked before the first termination.
    assert_eq!(fake.done[1..3], ["snapshot", "mark 1"]);
    assert!(fake.logged[0].1.contains("40 left"), "{:?}", fake.logged);
}

#[test]
fn users_that_cant_be_marked_are_their_own_exit_after_the_stop() {
    for marks in [Marks::LockBusy, Marks::WriteFails] {
        let mut fake = Fake::new(&[1010, 1011]);
        fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
        fake.marks = marks;
        assert_eq!(stop(&mut fake), StopExit::NotMarked);
        assert_eq!(
            fake.stopped(),
            [10, 12, 16],
            "stopped first, whatever the marks"
        );
        assert_eq!(fake.resets, Vec::<Sid>::new());
        assert!(!fake.held);
    }
    assert_eq!(StopExit::NotMarked.as_code(), 49);
}

#[test]
fn nothing_running_marks_no_one_and_takes_the_lock_only_to_record_the_completion() {
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.stopped(), Vec::<u32>::new());
    let done = [
        "set StopNext",
        "snapshot",
        "jobs",
        "set StopCompleted",
        "set StopSequence",
    ];
    assert_eq!(fake.done, done);
    assert_eq!(fake.resets, Vec::<Sid>::new());
    // With the lock held once reserved, the completion can't be written: 49, not 0.
    let mut fake = Fake::new(&[1010]);
    fake.marks = Marks::LockBusy;
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.done, ["set StopNext", "snapshot", "jobs"]);
    assert_eq!(fake.completed(), None);
}

#[test]
fn the_refusals_stop_nothing() {
    let refused = |fake: &mut Fake, exit: StopExit, case: &str| {
        fake.snapshots = vec![Ok(busy())];
        assert_eq!(stop(fake), exit, "{case}");
        assert!(fake.done.is_empty(), "{case}: {:?}", fake.done);
        assert!(fake.resets.is_empty(), "{case}");
        assert!(fake.logged[0].0, "{case}");
    };
    let mut fake = Fake::new(&[1010]);
    fake.system = false;
    refused(&mut fake, StopExit::NotAllowed, "not SYSTEM");
    let mut fake = Fake::new(&[1010]);
    fake.exe = PathBuf::from(r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe");
    refused(&mut fake, StopExit::NotAllowed, "not the admin copy");
    let mut fake = Fake::new(&[1010]);
    fake.pool = Err("PoolSids is malformed".to_owned());
    refused(&mut fake, StopExit::NoRecord, "no record");
    let mut fake = Fake::new(&[1010, 500]);
    fake.not_pool = true;
    refused(&mut fake, StopExit::NotPool, "not a pool user");
    // No record, or an empty pool, is nothing to stop: not even a snapshot.
    for pool in [None, Some(Vec::new())] {
        let mut fake = Fake::new(&[]);
        fake.pool = Ok(pool.map(|users| Pool {
            owner: Sid::parse(OWNER).unwrap(),
            users,
        }));
        fake.snapshots = vec![Ok(busy())];
        assert_eq!(stop(&mut fake), StopExit::Done);
        assert!(fake.done.is_empty(), "{:?}", fake.done);
    }
}

#[test]
fn the_exit_codes_follow_the_refresh_codes_and_are_never_shared() {
    use crate::windows::refresh::RefreshExit;
    let all = [
        StopExit::Done,
        StopExit::NotAllowed,
        StopExit::NoRecord,
        StopExit::NotPool,
        StopExit::NotStopped,
        StopExit::NotMarked,
        StopExit::NotReserved,
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 45, 46, 47, 48, 49, 50]);
    let refresh = RefreshExit::LockHeld.as_code();
    assert_eq!(refresh, 44, "the refresh's last code");
    let mut by_code = BTreeMap::new();
    for exit in all {
        assert!(by_code.insert(exit.as_code(), exit).is_none());
    }
    assert!(codes[1..].iter().all(|code| *code > refresh));
}
