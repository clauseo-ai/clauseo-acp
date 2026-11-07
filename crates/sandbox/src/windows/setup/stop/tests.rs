//! `--stop` through a fake machine, on every OS: the refusals stop nothing, only the record's
//! pool users' processes are stopped, twice over, and the users found are marked for a reset.

use std::collections::BTreeMap;

use super::*;

const OWNER: &str = "S-1-5-21-1-2-3-1001";
const ADMIN_EXE: &str = r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe";
const OWN_PID: u32 = 900;

fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-1-2-3-{rid}")).unwrap()
}

/// How the fake's marking goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marks {
    /// Written.
    Written,
    /// The set-up lock stays held.
    LockBusy,
    /// The write fails.
    WriteFails,
}

/// The fake: the record, the processes each snapshot sees, and everything done, in order.
struct Fake {
    system: bool,
    exe: PathBuf,
    pool: Result<Option<Pool>, Failed>,
    /// The pool-user check refuses.
    not_pool: bool,
    /// Each snapshot, in turn; past the last, the last again.
    snapshots: Vec<Result<Vec<Seen>, Failed>>,
    taken: usize,
    /// Pids whose termination fails.
    refuse: Vec<u32>,
    /// Pids whose first termination fails.
    refuse_once: Vec<u32>,
    /// Why the marks fail, if they do.
    marks: Marks,
    held: bool,
    /// The record's `ResetSids`.
    resets: Vec<Sid>,
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
            refuse: Vec::new(),
            refuse_once: Vec::new(),
            marks: Marks::Written,
            held: false,
            resets: Vec::new(),
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
}

fn seen(pid: u32, user: Option<Sid>) -> Seen {
    Seen { pid, user }
}

impl Stopper for Fake {
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
        self.pool.clone()
    }

    fn check_pool(&mut self, _pool: &Pool) -> Result<(), Failed> {
        if self.not_pool {
            return Err("S-1-5-21-1-2-3-500 isn't a pool user".to_owned());
        }
        Ok(())
    }

    fn snapshot(&mut self) -> Result<Vec<Seen>, Failed> {
        self.done.push("snapshot".to_owned());
        let at = self.taken.min(self.snapshots.len().saturating_sub(1));
        self.taken += 1;
        self.snapshots.get(at).cloned().unwrap_or(Ok(Vec::new()))
    }

    fn own_pid(&mut self) -> u32 {
        OWN_PID
    }

    fn terminate(&mut self, pid: u32) -> Result<(), Failed> {
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

    fn lock(&mut self) -> Result<bool, Failed> {
        assert!(!self.held, "taken once");
        self.held = self.marks != Marks::LockBusy;
        Ok(self.held)
    }

    fn unlock(&mut self) {
        self.held = false;
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

    fn log(&mut self, error: bool, line: &str) {
        self.logged.push((error, line.to_owned()));
    }
}

/// A machine where pool users 1010 and 1011 run, beside the owner, SYSTEM, another local
/// account, an unreadable process, this process, and the kernel's two.
fn busy() -> Vec<Seen> {
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
    assert_eq!(
        fake.done.iter().filter(|line| *line == "snapshot").count(),
        2
    );
    assert_eq!(fake.resets, [sid(1010), sid(1011)], "1012 wasn't running");
    assert!(!fake.held, "the lock is given back");
    let (error, line) = &fake.logged[0];
    assert!(!error);
    assert_eq!(
        line,
        "the sandbox's stop: 3 pool users, 2 found running, 3 processes stopped, 0 left; \
         2 users marked for a profile reset (0 retried)"
    );
    assert!(!line.contains("S-1-5"), "counts only: {line}");
}

#[test]
fn the_second_pass_stops_what_the_first_left_or_what_started_since() {
    let mut fake = Fake::new(&[1010, 1011]);
    // 10 is refused once in the first pass; the second finds it again, with 20, new since.
    fake.refuse_once = vec![10];
    let second = vec![seen(10, Some(sid(1010))), seen(20, Some(sid(1011)))];
    fake.snapshots = vec![Ok(busy()), Ok(second)];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.stopped(), [12, 16, 10, 20]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert!(
        fake.logged[0].1.contains("(1 retried)"),
        "{:?}",
        fake.logged
    );
}

#[test]
fn a_process_still_there_after_the_second_pass_is_not_stopped_but_its_user_is_marked() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.refuse = vec![12];
    fake.snapshots = vec![Ok(busy())];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(StopExit::NotStopped.as_code(), 48);
    assert_eq!(fake.stopped(), [10, 16, 10, 16]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("1 left"), "{line}");
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
fn nothing_running_takes_no_lock_and_marks_no_one() {
    let mut fake = Fake::new(&[1010]);
    fake.marks = Marks::LockBusy;
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.stopped(), Vec::<u32>::new());
    assert_eq!(fake.done, ["snapshot", "snapshot"]);
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
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 45, 46, 47, 48, 49]);
    let refresh = RefreshExit::LockHeld.as_code();
    assert_eq!(refresh, 44, "the refresh's last code");
    let mut by_code = BTreeMap::new();
    for exit in all {
        assert!(by_code.insert(exit.as_code(), exit).is_none());
    }
    assert!(codes[1..].iter().all(|code| *code > refresh));
}
