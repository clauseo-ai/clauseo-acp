//! The sweep through a fake process list: who is killed, what fails closed, and the job's
//! processes kept only by a sweep given the job.

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
}

fn pool_proc(image: &'static str) -> Proc {
    Proc {
        owner: Owner::User(sid(POOL)),
        image,
        in_job: false,
    }
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
                owner: p.owner.clone(),
            })
            .collect())
    }
    fn open(&mut self, pid: u32) -> io::Result<Option<u32>> {
        if self.deny_open.contains(&pid) {
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
    fake.procs.insert(
        0,
        Proc {
            owner: Owner::None,
            image: "[System Process]",
            in_job: false,
        },
    );
    fake.procs.insert(
        4,
        Proc {
            owner: Owner::User(sid("S-1-5-18")),
            image: "System",
            in_job: false,
        },
    );
    fake.procs.insert(
        900,
        Proc {
            owner: Owner::User(sid(LAWYER)),
            image: "lawyer.exe",
            in_job: false,
        },
    );
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
        let kernel = Proc {
            owner: Owner::None,
            image,
            in_job: false,
        };
        fake.procs.insert(pid, kernel);
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
    let lawyers = Proc {
        owner: Owner::User(sid(LAWYER)),
        image: "editor.exe",
        in_job: false,
    };
    fake.reused.insert(12, lawyers);
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
