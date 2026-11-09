//! The end sweeper's creation-time cutoff, the children of what it killed and its second pass,
//! and the escapees any sweep reports, through the same fake.

use super::*;

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
        type Process = Handle;
        fn list(&mut self) -> io::Result<Vec<Seen>> {
            self.0.list()
        }
        fn named(&mut self) -> io::Result<Vec<Seen>> {
            self.0.named()
        }
        fn open(&mut self, pid: u32) -> io::Result<Option<Handle>> {
            self.0.open(pid)
        }
        fn job_pids(&mut self) -> io::Result<Vec<u32>> {
            self.0.job_pids()
        }
        fn started(&mut self, process: &Handle) -> io::Result<u64> {
            self.0.started(process)
        }
        fn own_start(&mut self) -> io::Result<u64> {
            Err(io::Error::from_raw_os_error(6))
        }
        fn kill(&mut self, process: &Handle) -> io::Result<()> {
            self.0.kill(process)
        }
        fn exited(&mut self, process: &Handle, wait: Duration) -> bool {
            self.0.exited(process, wait)
        }
        fn pause(&mut self, pause: Duration) {
            self.0.pause(pause);
        }
        fn now(&mut self) -> Duration {
            self.0.now()
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
    let job = SweepError::JobList(code.clone());
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
