//! The sweep through a fake process list: who is killed, what fails closed, the job's
//! processes kept only by a sweep given the job, the second listing, and the end sweeper's
//! creation-time cutoff, the children of what it killed, and its second pass.

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
    /// The pid that created it, as the snapshot gives it.
    parent: u32,
}

fn proc(owner: Owner, image: &'static str) -> Proc {
    Proc {
        owner,
        image,
        in_job: false,
        started: 100,
        hidden: false,
        parent: 4,
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
    /// The opening list's nth call (from 1) finds these processes started meanwhile.
    appear: BTreeMap<usize, Vec<(u32, Proc)>>,
    /// How many times the opening list was read.
    lists: usize,
}

impl Processes for Fake {
    type Process = u32;
    fn list(&mut self) -> io::Result<Vec<Seen>> {
        self.lists += 1;
        for (pid, proc) in self.appear.remove(&self.lists).unwrap_or_default() {
            self.procs.insert(pid, proc);
        }
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
                parent: Some(p.parent),
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
                parent: None,
            })
            .chain([Seen {
                pid: 0,
                image: "System Idle Process".to_owned(),
                owner: Owner::None,
                parent: None,
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
    assert_eq!(
        got,
        Err(SweepError::OwnStart {
            code: "6".to_owned()
        })
    );
    assert_eq!(calls.0.killed, Vec::<u32>::new());
    // Not an escapee: no process of the pool user's was found unkillable.
    assert_eq!(got.unwrap_err().escapees(), None);
}

/// A process of the pool user created at `started` by `parent`.
fn child(image: &'static str, started: u64, parent: u32) -> Proc {
    Proc {
        started,
        parent,
        ..pool_proc(image)
    }
}

#[test]
fn the_end_sweeper_kills_what_a_process_it_killed_started_after_it_started() {
    // The sweeper started at 1000. Killing the old agent sets off a child (created at 1200, its
    // parent the old agent), which the cutoff alone would spare; the child's own child goes too.
    // The next session's inner (its parent the next outer, 900 the lawyer's) and agent are kept.
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    fake.procs.insert(31, child("old-agent.exe", 800, 4));
    fake.spawns.insert(31, (50, child("forked.exe", 1_200, 31)));
    fake.spawns
        .insert(50, (51, child("grandchild.exe", 1_300, 50)));
    fake.procs.insert(40, child("next-inner.exe", 1_500, 900));
    fake.procs.insert(41, child("next-agent.exe", 1_600, 40));
    let (got, _) = run(&mut fake, &[30], Point::Sweeper);
    assert_eq!(got, Ok(vec![31, 50, 51]));
    assert!(fake.procs.contains_key(&40) && fake.procs.contains_key(&41));
    // 20 is killed, then 31, its child (created at 1100). 60 (created at 1050) names 31 as its
    // parent, but an earlier process had pid 31 then: it isn't the killed one's child, and is
    // kept.
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    fake.procs.insert(20, child("old.exe", 800, 4));
    fake.procs.insert(31, child("forked.exe", 1_100, 20));
    fake.procs.insert(60, child("next.exe", 1_050, 31));
    assert_eq!(run(&mut fake, &[30], Point::Sweeper).0, Ok(vec![20, 31]));
    assert!(fake.procs.contains_key(&60));
    // A process created after the sweeper by one it didn't kill is kept.
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    fake.procs.insert(31, child("next-inner.exe", 1_100, 900));
    fake.procs.insert(60, child("next-agent.exe", 1_150, 31));
    assert_eq!(run(&mut fake, &[30], Point::Sweeper).0, Ok(vec![]));
}

#[test]
fn the_end_sweepers_second_pass_kills_what_got_past_the_first() {
    // The first pass kills 31 and its listing after that finds nothing new. A child 31 started
    // shows only in the next listing, the second pass's first: that pass kills it, knowing 31.
    let setup = || {
        let mut fake = machine();
        fake.procs.insert(30, pool_proc("sweeper.exe"));
        fake.procs.insert(31, child("old-agent.exe", 800, 4));
        // Lists: 1 and 2 are the first round's two (31 killed), 3 the quiet round's.
        fake.appear
            .insert(4, vec![(52, child("late-child.exe", 1_200, 31))]);
        fake
    };
    let mut fake = setup();
    let mut lines = Vec::new();
    let got = end_sweep(&mut fake, &sid(POOL), &[30], &mut |l| {
        lines.push(l.to_owned());
    });
    assert_eq!(got, Ok(vec![31, 52]));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[1].contains("process 52 (late-child.exe)"),
        "{lines:?}"
    );
    // One pass alone leaves it; a fresh sweep, not knowing 31, spares it as created after.
    let mut fake = setup();
    assert_eq!(run(&mut fake, &[30], Point::Sweeper).0, Ok(vec![31]));
    assert_eq!(run(&mut fake, &[30], Point::Sweeper).0, Ok(vec![]));
    assert!(fake.procs.contains_key(&52));
}

#[test]
fn the_end_sweeper_gives_its_second_pass_result() {
    // Left by both passes: the second pass's failure, the first's logged.
    let mut fake = machine();
    fake.procs.insert(
        12,
        Proc {
            hidden: true,
            ..pool_proc("hidden.exe")
        },
    );
    let mut lines = Vec::new();
    let got = end_sweep(&mut fake, &sid(POOL), &[30], &mut |l| {
        lines.push(l.to_owned());
    });
    assert!(
        matches!(got, Err(SweepError::Left { count: 1, .. })),
        "{got:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with(
            "clauseo-sandbox-run: sweep (end sweeper), first pass: 1 of the sandbox user's"
        )),
        "{lines:?}"
    );
    // Ended by the second pass: no failure.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("stuck.exe"));
    fake.deny_kill.insert(12, 1);
    let got = end_sweep(&mut fake, &sid(POOL), &[30], &mut |_| {});
    assert_eq!(got, Ok(vec![12]));
}

#[test]
fn a_process_that_cant_be_opened_or_ended_is_an_escapee_and_one_that_cant_be_checked_isnt() {
    let one = |image: &str| {
        Some(Escapees {
            count: 1,
            images: vec![image.to_owned()],
        })
    };
    let (pid, code) = (12, "5".to_owned());
    let image = "hidden.exe".to_owned();
    let open = SweepError::Open {
        pid,
        image: image.clone(),
        code: code.clone(),
    };
    let kill = SweepError::Kill {
        pid,
        image: image.clone(),
        code: code.clone(),
    };
    assert_eq!(open.escapees(), one("hidden.exe"));
    assert_eq!(kill.escapees(), one("hidden.exe"));
    let job = SweepError::Job {
        pid,
        image: image.clone(),
        code: code.clone(),
    };
    let started = SweepError::Started { pid, image, code };
    for plain in [
        job,
        started,
        SweepError::List("5".to_owned()),
        SweepError::StillRunning,
        SweepError::Left {
            count: 2,
            unkillable: 0,
            images: Vec::new(),
        },
    ] {
        assert_eq!(plain.escapees(), None, "{plain:?}");
    }
    // The first act fails at the first one.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("hidden.exe"));
    fake.deny_open.insert(12);
    let got = run(&mut fake, &[], Point::Start).0.unwrap_err();
    assert_eq!(got.escapees(), one("hidden.exe"));
}

#[test]
fn a_later_sweep_counts_its_escapees_and_names_each_image_once_up_to_the_cap() {
    for point in [Point::Tick, Point::End, Point::Sweeper] {
        let mut fake = machine();
        fake.job = point != Point::Sweeper;
        // Two hidden copies of one image, nine others, and one that only can't be checked.
        for pid in 20..22 {
            fake.procs.insert(
                pid,
                Proc {
                    hidden: true,
                    ..pool_proc("same.exe")
                },
            );
        }
        let images = [
            "a.exe", "b.exe", "c.exe", "d.exe", "e.exe", "f.exe", "g.exe", "h.exe", "i.exe",
        ];
        for (pid, image) in (30..).zip(images) {
            fake.procs.insert(pid, pool_proc(image));
            fake.deny_kill.insert(pid, u32::MAX);
        }
        let got = run(&mut fake, &[], point).0.unwrap_err();
        let escapees = got.escapees().unwrap();
        assert_eq!(escapees.count, 11, "{point}");
        assert_eq!(escapees.images.len(), IMAGES_CAP, "{point}");
        assert_eq!(escapees.images[..2], ["same.exe", "a.exe"], "{point}");
    }
}

#[test]
fn the_end_sweepers_exit_code_says_escapee_sweep_or_nothing() {
    let escapee = crate::windows::runner::RunExit::Escapee.code();
    let sweep_code = crate::windows::runner::RunExit::Sweep.code();
    // A process that shut its DACL outlives both passes: Escapee, whatever the removal did.
    let mut fake = machine();
    fake.procs.insert(
        12,
        Proc {
            hidden: true,
            ..pool_proc("hidden.exe")
        },
    );
    let kills = end_sweep(&mut fake, &sid(POOL), &[30], &mut |_| {}).map(drop);
    assert_eq!(end_sweeper_exit(&kills, true), escapee);
    assert_eq!(end_sweeper_exit(&kills, false), escapee);
    // Any other failure, of the kills or of the removal: Sweep.
    assert_eq!(
        end_sweeper_exit(&Err(SweepError::StillRunning), true),
        sweep_code
    );
    assert_eq!(end_sweeper_exit(&Ok(()), false), sweep_code);
    assert_eq!(end_sweeper_exit(&Ok(()), true), 0);
}
