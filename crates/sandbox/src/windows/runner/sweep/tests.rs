//! The sweep through a fake process list: who is killed, what fails closed, the job's processes
//! kept past the first act by the job's pid list and the creation-time rule, and the second
//! listing.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const LAWYER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1500";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// One process of the fake machine.
#[derive(Clone)]
struct Proc {
    owner: Owner,
    image: &'static str,
    /// Its pid is in the job's pid list.
    in_job: bool,
    /// Its creation time.
    started: u64,
    /// Its DACL shuts its user out: the opening list can't read its user, and only the service's
    /// list names it.
    hidden: bool,
}

fn proc(owner: Owner, image: &'static str) -> Proc {
    Proc {
        owner,
        image,
        in_job: false,
        started: 100,
        hidden: false,
    }
}

fn pool_proc(image: &'static str) -> Proc {
    proc(Owner::User(sid(POOL)), image)
}

/// What a kill that works does to the fake's process.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Ends {
    /// It is gone at once, from the lists too.
    #[default]
    Gone,
    /// It has exited, but stays listed (`dead`) while the sweep holds it open.
    Listed,
    /// It ends only once a wait on it has run its whole length: its kill takes time.
    Slowly,
}

/// A machine of processes; each call is recorded.
#[derive(Default)]
struct Fake {
    procs: BTreeMap<u32, Proc>,
    /// Opening this pid is refused.
    deny_open: BTreeSet<u32>,
    /// Ending this pid fails this many more times.
    deny_kill: BTreeMap<u32, u32>,
    /// Killing the key starts the value: a child it started just before.
    spawns: BTreeMap<u32, (u32, Proc)>,
    /// Opening the key finds the pid given to another user's process meanwhile.
    reused: BTreeMap<u32, Proc>,
    /// Every pid killed, in order.
    killed: Vec<u32>,
    /// A process that starts another each time it is killed.
    forever: Option<u32>,
    /// How many times the job's pid list was read.
    job_reads: usize,
    /// Reading the job's pid list fails.
    job_list_fails: bool,
    /// The sweeping process's own creation time.
    own_start: u64,
    /// Opening a hidden process with `WRITE_DAC` and granting itself works (no `OWNER RIGHTS`
    /// entry): its next open succeeds.
    regrant: bool,
    /// The opening list's nth call (from 1) finds these processes started meanwhile.
    appear: BTreeMap<usize, Vec<(u32, Proc)>>,
    /// The opening list's nth call (from 1) no longer finds these pids.
    gone_at: BTreeMap<usize, Vec<u32>>,
    /// From the opening list's nth call (from 1), neither list names these pids' user.
    unnamed_at: BTreeMap<usize, Vec<u32>>,
    /// How many times the opening list was read.
    lists: usize,
    /// This pid exits by itself after this many pauses.
    exits_after: BTreeMap<u32, u32>,
    /// Every pause the sweep made, in order.
    pauses: Vec<Duration>,
    /// Asking this pid to end "works" (the call succeeds) but leaves it running this many more
    /// times (`u32::MAX`: always): a process frozen, or held in the kernel.
    stays: BTreeMap<u32, u32>,
    /// These pids have exited but are still listed and can be opened: another process holds
    /// them open. Their kill is refused (5), as Windows refuses one for an exited process.
    dead: BTreeSet<u32>,
    /// What a kill that works does to the process.
    ends: Ends,
    /// Every kill asked for, in order, whether or not it ended the process.
    asked: Vec<u32>,
    /// Every wait longer than none on a process's end, in order.
    waits: Vec<Duration>,
    /// The clock: each pause, and each wait on a process that doesn't end within it, moves it on.
    clock: Duration,
    /// Once asked to end, this pid is in neither list, whether or not it ended.
    vanish: BTreeSet<u32>,
    /// Killed processes not yet ended (`Ends::Slowly`).
    ending: BTreeSet<u32>,
}

/// An open process: its pid and creation time, as a handle names one process and no later one
/// that gets its pid.
type Handle = (u32, u64);

impl Fake {
    /// Whether `pid` was asked to end and is to vanish from the lists.
    fn vanished(&self, pid: u32) -> bool {
        self.vanish.contains(&pid) && self.asked.contains(&pid)
    }

    /// Whether the process `handle` names is still the one at its pid, and running.
    fn running(&self, (pid, started): Handle) -> bool {
        self.procs.get(&pid).is_some_and(|p| p.started == started) && !self.dead.contains(&pid)
    }
}

impl Processes for Fake {
    type Process = Handle;
    fn list(&mut self) -> io::Result<Vec<Seen>> {
        self.lists += 1;
        for (pid, proc) in self.appear.remove(&self.lists).unwrap_or_default() {
            self.procs.insert(pid, proc);
        }
        for pid in self.gone_at.remove(&self.lists).unwrap_or_default() {
            self.procs.remove(&pid);
        }
        for pid in self.unnamed_at.remove(&self.lists).unwrap_or_default() {
            if let Some(p) = self.procs.get_mut(&pid) {
                p.owner = Owner::None;
            }
        }
        Ok(self
            .procs
            .iter()
            .filter(|(pid, _)| !self.vanished(**pid))
            .map(|(pid, p)| Seen {
                pid: *pid,
                image: p.image.to_owned(),
                owner: if p.hidden {
                    Owner::None
                } else {
                    p.owner.clone()
                },
            })
            .collect())
    }
    fn named(&mut self) -> io::Result<Vec<Seen>> {
        // Only this user's own processes, with no SID for the kernel's.
        Ok(self
            .procs
            .iter()
            .filter(|(pid, p)| {
                matches!(&p.owner, Owner::User(u) if *u == sid(POOL)) && !self.vanished(**pid)
            })
            .map(|(pid, p)| Seen {
                pid: *pid,
                image: p.image.to_owned(),
                owner: p.owner.clone(),
            })
            .chain([Seen {
                pid: 0,
                image: "System Idle Process".to_owned(),
                owner: Owner::None,
            }])
            .collect())
    }
    fn open(&mut self, pid: u32) -> io::Result<Option<Handle>> {
        let hidden = self.procs.get(&pid).is_some_and(|p| p.hidden);
        if hidden && self.regrant {
            if let Some(p) = self.procs.get_mut(&pid) {
                p.hidden = false;
            }
        } else if self.deny_open.contains(&pid) || hidden {
            return Err(io::Error::from_raw_os_error(5));
        }
        if let Some(other) = self.reused.remove(&pid) {
            self.procs.insert(pid, other);
        }
        Ok(self.procs.get(&pid).map(|p| (pid, p.started)))
    }
    fn job_pids(&mut self) -> io::Result<Vec<u32>> {
        self.job_reads += 1;
        if self.job_list_fails {
            return Err(io::Error::from_raw_os_error(6));
        }
        Ok(self
            .procs
            .iter()
            .filter(|(_, p)| p.in_job)
            .map(|(pid, _)| *pid)
            .collect())
    }
    fn started(&mut self, process: &Handle) -> io::Result<u64> {
        // A handle reads its own process's time, whatever has the pid since.
        Ok(process.1)
    }
    fn own_start(&mut self) -> io::Result<u64> {
        Ok(self.own_start)
    }
    fn kill(&mut self, process: &Handle) -> io::Result<()> {
        let pid = &process.0;
        self.asked.push(*pid);
        if !self.running(*process) {
            // Windows refuses to end what has exited.
            return Err(io::Error::from_raw_os_error(5));
        }
        if let Some(left) = self.stays.get_mut(pid)
            && *left > 0
        {
            *left = left.saturating_sub(1);
            return Ok(());
        }
        if let Some(left) = self.deny_kill.get_mut(pid)
            && *left > 0
        {
            *left -= 1;
            return Err(io::Error::from_raw_os_error(5));
        }
        match self.ends {
            Ends::Slowly => {
                self.ending.insert(*pid);
            }
            Ends::Listed => {
                self.dead.insert(*pid);
            }
            Ends::Gone => {
                self.procs.remove(pid);
            }
        }
        self.killed.push(*pid);
        if let Some((child, proc)) = self.spawns.remove(pid) {
            self.procs.insert(child, proc);
        }
        if self.forever == Some(*pid) {
            let next = pid + 1;
            self.procs.insert(next, pool_proc("again.exe"));
            self.forever = Some(next);
        }
        Ok(())
    }
    fn exited(&mut self, process: &Handle, wait: Duration) -> bool {
        if !wait.is_zero() {
            self.waits.push(wait);
        }
        if self.ending.contains(&process.0) {
            if wait.is_zero() {
                return false;
            }
            self.clock += wait;
            self.ending.remove(&process.0);
            self.procs.remove(&process.0);
            return true;
        }
        let running = self.running(*process);
        if running {
            self.clock += wait;
        }
        !running
    }
    fn now(&mut self) -> Duration {
        self.clock
    }
    fn pause(&mut self, pause: Duration) {
        self.clock += pause;
        self.pauses.push(pause);
        for (pid, left) in &mut self.exits_after {
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.procs.remove(pid);
            }
        }
    }
}

fn machine() -> Fake {
    let mut fake = Fake::default();
    fake.procs.insert(0, proc(Owner::None, "[System Process]"));
    fake.procs
        .insert(4, proc(Owner::User(sid("S-1-5-18")), "System"));
    fake.procs
        .insert(900, proc(Owner::User(sid(LAWYER)), "lawyer.exe"));
    fake.own_start = 1_000;
    fake
}

/// The agent: in the job, created after the inner (`machine`'s `own_start`).
fn agent() -> Proc {
    Proc {
        in_job: true,
        started: 1_100,
        ..pool_proc("agent.exe")
    }
}

fn run(fake: &mut Fake, keep: &[u32], point: Point) -> (Result<Vec<u32>, SweepError>, Vec<String>) {
    let mut lines = Vec::new();
    let got = sweep(fake, &sid(POOL), keep, point, &mut |l| {
        lines.push(l.to_owned());
    });
    (got, lines)
}

#[test]
fn only_the_pool_users_processes_outside_the_job_are_killed() {
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    fake.procs.insert(11, agent());
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.procs.insert(13, pool_proc("bits-job.exe"));
    let (got, lines) = run(&mut fake, &[10], Point::Tick);
    assert_eq!(got, Ok(vec![12, 13]));
    assert_eq!(fake.killed, [12, 13]);
    for pid in [0, 4, 900, 10, 11] {
        assert!(fake.procs.contains_key(&pid), "{pid} kept");
    }
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: sweep (mid-session) killed the sandbox user's process 12 \
             (stray.exe)",
            "clauseo-sandbox-run: sweep (mid-session) killed the sandbox user's process 13 \
             (bits-job.exe)",
        ]
    );
}

#[test]
fn nothing_of_the_pool_user_is_nothing_to_do() {
    let mut fake = machine();
    assert_eq!(run(&mut fake, &[], Point::Start).0, Ok(vec![]));
    assert_eq!(fake.killed, Vec::<u32>::new());
}

#[test]
fn entries_without_a_user_are_passed_over_and_the_pool_users_still_fail_closed() {
    let mut fake = machine();
    for (pid, image) in [
        (88, "Registry"),
        (160, "Secure System"),
        (2100, "MemCompression"),
    ] {
        fake.procs.insert(pid, proc(Owner::None, image));
    }
    fake.procs.insert(12, pool_proc("stray.exe"));
    assert_eq!(run(&mut fake, &[], Point::Start).0, Ok(vec![12]));
    for pid in [0, 88, 160, 2100] {
        assert!(fake.procs.contains_key(&pid), "{pid} passed over");
    }
    // Beside them, a pool user's process that can't be opened still fails the launch.
    fake.procs.insert(13, pool_proc("hidden.exe"));
    fake.deny_open.insert(13);
    let got = run(&mut fake, &[], Point::Start).0;
    assert!(
        matches!(&got, Err(SweepError::Open { pid: 13, code, .. }) if code == "5"),
        "{got:?}"
    );
}

#[test]
fn a_process_that_cant_be_opened_or_ended_fails_closed() {
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.deny_open.insert(12);
    let got = run(&mut fake, &[], Point::Start).0;
    assert!(
        matches!(&got, Err(SweepError::Open { pid: 12, code, .. }) if code == "5"),
        "{got:?}"
    );
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.deny_kill.insert(12, 1);
    let got = run(&mut fake, &[], Point::Start).0;
    assert!(
        matches!(&got, Err(SweepError::Kill { pid: 12, .. })),
        "{got:?}"
    );
    assert!(fake.procs.contains_key(&12));
}

#[test]
fn a_pid_reused_by_another_user_before_the_open_is_left_alone() {
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.reused
        .insert(12, proc(Owner::User(sid(LAWYER)), "editor.exe"));
    assert_eq!(run(&mut fake, &[], Point::Start).0, Ok(vec![]));
    assert_eq!(fake.killed, Vec::<u32>::new());
    assert!(fake.procs.contains_key(&12));
}

#[test]
fn what_a_killed_process_started_is_killed_in_the_next_round() {
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.spawns.insert(12, (40, pool_proc("child.exe")));
    assert_eq!(run(&mut fake, &[], Point::End).0, Ok(vec![12, 40]));
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("hydra.exe"));
    fake.forever = Some(12);
    let got = run(&mut fake, &[], Point::Start).0;
    assert_eq!(got, Err(SweepError::StillRunning));
}

#[test]
fn the_first_act_keeps_only_its_own_pid() {
    // The inner's first act: a process in the job is killed too. Nothing but the inner should
    // be in it yet, and one there would have been started from the inner's process by one that
    // opened it before the shut.
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    fake.procs.insert(11, agent());
    let (got, lines) = run(&mut fake, &[10], Point::Start);
    assert_eq!(got, Ok(vec![11]));
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: sweep (before launch) killed the sandbox user's process 11 \
          (agent.exe)"
        ]
    );
    // Any process created before the inner, a runner of an earlier session's among them, goes.
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    fake.procs.insert(30, pool_proc("clauseo-sandbox-run.exe"));
    assert_eq!(run(&mut fake, &[10], Point::Start).0, Ok(vec![30]));
}

#[test]
fn a_process_that_shut_its_dacl_is_found_by_the_service_list_and_fails_the_first_act() {
    // The opening list reads no user for it; the service's list names the pool user.
    let mut fake = machine();
    fake.procs.insert(
        12,
        Proc {
            hidden: true,
            ..pool_proc("hidden.exe")
        },
    );
    let got = run(&mut fake, &[], Point::Start).0;
    assert!(
        matches!(&got, Err(SweepError::Open { pid: 12, code, .. }) if code == "5"),
        "{got:?}"
    );
    // The service's entry with no SID is passed over.
    assert!(fake.procs.contains_key(&0));
    // As its owner the sweep may grant itself the rights first, when no OWNER RIGHTS entry
    // stops it: then it is killed.
    fake.regrant = true;
    assert_eq!(run(&mut fake, &[], Point::Start).0, Ok(vec![12]));
    assert!(fake.procs.contains_key(&0));
}

#[test]
fn past_the_first_act_a_failure_is_logged_once_and_the_rest_are_swept() {
    for point in [Point::Tick, Point::End] {
        let mut fake = machine();
        fake.procs.insert(
            12,
            Proc {
                hidden: true,
                ..pool_proc("hidden.exe")
            },
        );
        fake.procs.insert(13, pool_proc("stray.exe"));
        // Killing it starts a child, so the sweep makes a second round and tries 12 again.
        fake.spawns.insert(13, (14, pool_proc("child.exe")));
        let (got, lines) = run(&mut fake, &[], point);
        let left = SweepError::Left {
            count: 1,
            unkillable: 1,
            images: vec!["hidden.exe".to_owned()],
        };
        assert_eq!(got, Err(left), "{point}");
        assert_eq!(fake.killed, [13, 14], "{point}");
        let failures: Vec<&String> = lines.iter().filter(|l| l.contains("hidden.exe")).collect();
        assert_eq!(
            failures,
            [&format!(
                "clauseo-sandbox-run: sweep ({point}): the sandbox user's process 12 \
                 (hidden.exe) can't be opened (5)"
            )],
            "{point}"
        );
    }
}

#[test]
fn the_job_keeps_a_listed_pid_created_no_earlier_than_the_inner() {
    let job = [11, 12];
    assert!(
        job_kept(&job, 11, 1_100, 1_000),
        "listed, created after the inner"
    );
    assert!(
        job_kept(&job, 11, 1_000, 1_000),
        "listed, created in the inner's tick"
    );
    assert!(
        !job_kept(&job, 12, 999, 1_000),
        "listed, created before: a reused pid"
    );
    assert!(!job_kept(&job, 13, 1_100, 1_000), "not listed");
    assert!(!job_kept(&[], 11, 1_100, 1_000), "no list");
}

#[test]
fn past_the_first_act_the_job_is_its_pid_list_read_each_round_after_the_opens() {
    for point in [Point::Tick, Point::End] {
        let mut fake = machine();
        fake.procs.insert(10, pool_proc("inner.exe"));
        fake.procs.insert(11, agent());
        // Listed, but created before the inner: no process of the job can be; killed.
        fake.procs.insert(
            12,
            Proc {
                in_job: true,
                started: 500,
                ..pool_proc("old.exe")
            },
        );
        fake.procs.insert(13, pool_proc("stray.exe"));
        // Killing 13 lets the agent's child 40 appear: the next round's list holds it.
        fake.spawns.insert(
            13,
            (
                40,
                Proc {
                    in_job: true,
                    started: 1_200,
                    ..pool_proc("agent-child.exe")
                },
            ),
        );
        let (got, _) = run(&mut fake, &[10], point);
        assert_eq!(got, Ok(vec![12, 13]), "{point}");
        assert!(fake.procs.contains_key(&11) && fake.procs.contains_key(&40));
        // Once a round that opened something: the first, then the one that found 40.
        assert_eq!(fake.job_reads, 2, "{point}");
    }
    // Nothing to open: no read.
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    assert_eq!(run(&mut fake, &[10], Point::Tick).0, Ok(vec![]));
    assert_eq!(fake.job_reads, 0);
}

#[test]
fn a_job_list_that_cant_be_read_fails_the_sweep() {
    // The first act reads it before anything, even with nothing to kill, and fails closed.
    let mut fake = machine();
    fake.job_list_fails = true;
    let got = run(&mut fake, &[10], Point::Start).0;
    assert_eq!(got, Err(SweepError::JobList("6".to_owned())));
    assert_eq!(got.unwrap_err().escapees(), None);
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.job_list_fails = true;
    assert!(run(&mut fake, &[], Point::Start).0.is_err());
    assert_eq!(fake.killed, Vec::<u32>::new(), "nothing killed before it");
    // A tick fails without killing on a guess; the next tick tries again.
    let mut fake = machine();
    fake.procs.insert(11, agent());
    fake.job_list_fails = true;
    let got = run(&mut fake, &[], Point::Tick).0;
    assert_eq!(got, Err(SweepError::JobList("6".to_owned())));
    assert_eq!(fake.killed, Vec::<u32>::new());
}

mod escapees;
mod kills;
mod refused;
