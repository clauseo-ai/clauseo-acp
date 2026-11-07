//! The sweep through a fake process list: who is killed, what fails closed, the job's
//! processes kept only by a sweep given the job, the second listing, and the end sweeper's
//! creation-time cutoff.

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
    /// Whether this sweep was given the job (the inner's tick and end); without it, nothing is
    /// in the job.
    job: bool,
    /// The sweeping process's own creation time.
    own_start: u64,
    /// Opening a hidden process with `WRITE_DAC` and granting itself works (no `OWNER RIGHTS`
    /// entry): its next open succeeds.
    regrant: bool,
}

impl Processes for Fake {
    type Process = u32;
    fn list(&mut self) -> io::Result<Vec<Seen>> {
        Ok(self
            .procs
            .iter()
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
            .filter(|(_, p)| matches!(&p.owner, Owner::User(u) if *u == sid(POOL)))
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
    fn open(&mut self, pid: u32) -> io::Result<Option<u32>> {
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
        Ok(self.procs.contains_key(&pid).then_some(pid))
    }
    fn in_job(&mut self, process: &u32) -> io::Result<bool> {
        Ok(self.job && self.procs.get(process).is_some_and(|p| p.in_job))
    }
    fn started(&mut self, process: &u32) -> io::Result<u64> {
        self.procs
            .get(process)
            .map(|p| p.started)
            .ok_or_else(|| io::Error::from_raw_os_error(87))
    }
    fn own_start(&mut self) -> io::Result<u64> {
        Ok(self.own_start)
    }
    fn kill(&mut self, process: &u32) -> io::Result<()> {
        let pid = process;
        if let Some(left) = self.deny_kill.get_mut(pid)
            && *left > 0
        {
            *left -= 1;
            return Err(io::Error::from_raw_os_error(5));
        }
        self.procs.remove(pid);
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
    fake.procs.insert(
        11,
        Proc {
            in_job: true,
            ..pool_proc("agent.exe")
        },
    );
    fake.procs.insert(12, pool_proc("stray.exe"));
    fake.procs.insert(13, pool_proc("bits-job.exe"));
    fake.job = true;
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
fn a_sweep_without_the_job_keeps_only_its_own_pid() {
    // The inner's first act and the end sweeper: a process marked in the job is killed too,
    // since nothing but the inner can be in it yet, or the job is gone.
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    fake.procs.insert(
        11,
        Proc {
            in_job: true,
            ..pool_proc("agent.exe")
        },
    );
    let (got, lines) = run(&mut fake, &[10], Point::Start);
    assert_eq!(got, Ok(vec![11]));
    assert_eq!(
        lines,
        [
            "clauseo-sandbox-run: sweep (before launch) killed the sandbox user's process 11 \
          (agent.exe)"
        ]
    );
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    fake.procs.insert(31, pool_proc("task.exe"));
    let (got, lines) = run(&mut fake, &[30], Point::Sweeper);
    assert_eq!(got, Ok(vec![31]));
    assert!(lines[0].contains("sweep (end sweeper) killed"), "{lines:?}");
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
    for point in [Point::Tick, Point::End, Point::Sweeper] {
        let mut fake = machine();
        fake.job = point != Point::Sweeper;
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
        assert_eq!(got, Err(SweepError::Left { count: 1 }), "{point}");
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
fn the_end_sweeper_kills_only_what_was_created_before_it() {
    // The sweeper started at 1000; a next session's inner and agent started later.
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    for (pid, image, started) in [
        (31, "old-agent.exe", 900),
        (32, "same-tick.exe", 1_000),
        (40, "next-inner.exe", 1_500),
        (41, "next-agent.exe", 1_600),
    ] {
        fake.procs.insert(
            pid,
            Proc {
                started,
                ..pool_proc(image)
            },
        );
    }
    let (got, lines) = run(&mut fake, &[30], Point::Sweeper);
    assert_eq!(got, Ok(vec![31]));
    assert_eq!(lines.len(), 1, "{lines:?}");
    for pid in [32, 40, 41] {
        assert!(fake.procs.contains_key(&pid), "{pid} left alone");
    }
    // The other sweeps take no cutoff: the next inner's first act kills all of them, the old
    // sweeper included.
    let (got, _) = run(&mut fake, &[40], Point::Start);
    assert_eq!(got, Ok(vec![30, 32, 41]));
}

#[test]
fn an_end_sweeper_that_cant_read_its_own_start_kills_nothing() {
    struct NoClock(Fake);
    impl Processes for NoClock {
        type Process = u32;
        fn list(&mut self) -> io::Result<Vec<Seen>> {
            self.0.list()
        }
        fn named(&mut self) -> io::Result<Vec<Seen>> {
            self.0.named()
        }
        fn open(&mut self, pid: u32) -> io::Result<Option<u32>> {
            self.0.open(pid)
        }
        fn in_job(&mut self, process: &u32) -> io::Result<bool> {
            self.0.in_job(process)
        }
        fn started(&mut self, process: &u32) -> io::Result<u64> {
            self.0.started(process)
        }
        fn own_start(&mut self) -> io::Result<u64> {
            Err(io::Error::from_raw_os_error(6))
        }
        fn kill(&mut self, process: &u32) -> io::Result<()> {
            self.0.kill(process)
        }
    }
    let mut fake = machine();
    fake.procs.insert(31, pool_proc("task.exe"));
    let mut calls = NoClock(fake);
    let got = sweep(&mut calls, &sid(POOL), &[30], Point::Sweeper, &mut |_| {});
    assert!(
        matches!(&got, Err(SweepError::Started { pid: 30, code, .. }) if code == "6"),
        "{got:?}"
    );
    assert_eq!(calls.0.killed, Vec::<u32>::new());
}
