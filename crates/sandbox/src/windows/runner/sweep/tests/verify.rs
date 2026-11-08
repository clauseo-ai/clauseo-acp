//! A kill is its handle signalled: a kill whose call succeeds but leaves the process running is
//! a survivor (an escapee: the slot is quarantined), never "killed"; an exited process still
//! listed is no kill and can't keep the rounds going; a process is known by its pid and creation
//! time; and a process in a job of its own is found by its user all the same.

use super::*;

const CHROME: &str = "chrome-headless-shell.exe";

fn killed_lines(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.contains(") killed ")).collect()
}

#[test]
fn a_kill_that_succeeds_but_leaves_the_process_running_is_a_survivor_and_quarantines_the_slot() {
    for point in [Point::Tick, Point::End, Point::Sweeper] {
        let mut fake = machine();
        fake.procs.insert(10, pool_proc("inner.exe"));
        fake.procs.insert(1628, pool_proc(CHROME));
        fake.stays.insert(1628, u32::MAX);
        let (got, lines) = run(&mut fake, &[10], point);
        assert_eq!(
            got,
            Err(SweepError::Left {
                count: 1,
                unkillable: 1,
                images: vec![CHROME.to_owned()],
            }),
            "{point}"
        );
        // Asked once, waited on for the bound, and never logged as killed.
        assert_eq!(fake.asked, [1628], "{point}");
        assert_eq!(fake.waits, [KILL_WAIT], "{point}");
        assert_eq!(fake.killed, Vec::<u32>::new(), "{point}");
        assert!(fake.procs.contains_key(&1628));
        assert_eq!(
            lines,
            [format!(
                "clauseo-sandbox-run: sweep ({point}): the sandbox user's process 1628 \
                 (chrome-headless-shell.exe) still runs 5 s after its kill"
            )],
            "{point}"
        );
        // An escapee: the run, and the end sweeper, exit RunExit::Escapee, so the slot isn't
        // freed.
        let error = got.unwrap_err();
        assert_eq!(
            error.escapees(),
            Some(Escapees {
                count: 1,
                images: vec![CHROME.to_owned()],
            })
        );
        assert_eq!(
            end_sweeper_exit(&Err(error), true),
            crate::windows::runner::RunExit::Escapee.code()
        );
    }
    // The first act fails closed on it.
    let mut fake = machine();
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.stays.insert(1628, u32::MAX);
    let got = run(&mut fake, &[], Point::Start).0;
    assert_eq!(
        got,
        Err(SweepError::Survived {
            pid: 1628,
            image: CHROME.to_owned(),
        })
    );
    assert!(got.unwrap_err().escapees().is_some());
}

#[test]
fn a_survivor_is_asked_again_each_round_without_the_wait_and_is_killed_once_it_ends() {
    // 13's kill lets 40 appear, so there are three rounds; 1628 never ends.
    let mut fake = machine();
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.stays.insert(1628, u32::MAX);
    fake.procs.insert(13, pool_proc("stray.exe"));
    fake.spawns.insert(13, (40, pool_proc("child.exe")));
    let (got, lines) = run(&mut fake, &[], Point::End);
    assert!(
        matches!(
            got,
            Err(SweepError::Left {
                count: 1,
                unkillable: 1,
                ..
            })
        ),
        "{got:?}"
    );
    assert_eq!(fake.killed, [13, 40]);
    assert_eq!(fake.asked, [13, 1628, 40, 1628, 1628]);
    // One bounded wait per kill asked for the first time, none for a survivor's retries.
    assert_eq!(fake.waits, [KILL_WAIT; 3]);
    assert_eq!(killed_lines(&lines).len(), 2, "{lines:?}");
    assert_eq!(
        lines.iter().filter(|l| l.contains("still runs")).count(),
        1,
        "logged once: {lines:?}"
    );
    // One that ends at its second ask is killed then, and the sweep passes.
    let mut fake = machine();
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.stays.insert(1628, 1);
    fake.procs.insert(13, pool_proc("stray.exe"));
    fake.spawns.insert(13, (40, pool_proc("child.exe")));
    let (got, lines) = run(&mut fake, &[], Point::End);
    assert_eq!(got, Ok(vec![13, 40, 1628]));
    assert!(!fake.procs.contains_key(&1628));
    assert!(
        lines.contains(
            &"clauseo-sandbox-run: sweep (session end) killed the sandbox user's process 1628 \
              (chrome-headless-shell.exe)"
                .to_owned()
        ),
        "{lines:?}"
    );
}

#[test]
fn a_killed_process_still_listed_is_not_killed_again_and_doesnt_keep_the_rounds_going() {
    // The VM's pattern: the lists keep giving a process the sweep killed (its object held open).
    // Each round "killing" it again was 8 "killed" lines and "still started processes after 8
    // rounds" a sweep.
    for point in [Point::Tick, Point::End, Point::Sweeper] {
        let mut fake = machine();
        fake.lists_the_dead = true;
        fake.procs.insert(1628, pool_proc(CHROME));
        let (got, lines) = run(&mut fake, &[], point);
        assert_eq!(got, Ok(vec![1628]), "{point}");
        assert_eq!(fake.asked, [1628], "{point}");
        assert_eq!(killed_lines(&lines).len(), 1, "{point}: {lines:?}");
        // The second round found it, knew it, and killed nothing.
        assert_eq!(fake.lists, 4, "{point}");
    }
}

#[test]
fn an_exited_process_another_holds_open_is_no_kill_and_is_logged_once() {
    // An exited process still listed, held open by its hung parent: nothing to end.
    let mut fake = machine();
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.dead.insert(1628);
    fake.procs.insert(13, pool_proc("stray.exe"));
    fake.spawns.insert(13, (40, pool_proc("child.exe")));
    let (got, lines) = run(&mut fake, &[], Point::Tick);
    assert_eq!(got, Ok(vec![13, 40]));
    assert_eq!(fake.asked, [13, 40], "no kill asked of an exited process");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("had already exited"))
            .collect::<Vec<_>>(),
        [
            "clauseo-sandbox-run: sweep (mid-session): the sandbox user's process 1628 \
             (chrome-headless-shell.exe) had already exited (another process holds it open)"
        ]
    );
    assert_eq!(killed_lines(&lines).len(), 2, "{lines:?}");
}

#[test]
fn the_end_sweeper_passes_over_a_wer_clone_that_exited_but_is_still_listed() {
    // The VM's process: WER's zero-thread clone of the crashed main, which the main holds open
    // until the job goes. Its handle is signalled: both passes leave it, log it once, and the
    // sweep passes, so the end sweeper opens its token to COM and lists the scheduled tasks
    // rather than exiting RunExit::Sweep.
    let mut fake = machine();
    fake.procs.insert(30, pool_proc("sweeper.exe"));
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.dead.insert(1628);
    let mut lines = Vec::new();
    let got = end_sweep(&mut fake, &sid(POOL), &[30], &mut |l| {
        lines.push(l.to_owned());
    });
    assert_eq!(got, Ok(vec![]));
    assert_eq!(fake.asked, Vec::<u32>::new());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("had already exited"), "{lines:?}");
    assert_eq!(end_sweeper_exit(&got.map(drop), true), 0);
}

#[test]
fn a_process_in_a_job_of_its_own_is_found_by_its_user_and_killed() {
    // Chrome's sandboxed child: its own job, which the inner's job's pid list doesn't name; its
    // DACL shuts its user out, so only the service's list names its user.
    for point in [Point::Tick, Point::End, Point::Sweeper] {
        let mut fake = machine();
        fake.regrant = true;
        fake.procs.insert(10, pool_proc("inner.exe"));
        fake.procs.insert(11, agent());
        fake.procs.insert(
            1628,
            Proc {
                hidden: true,
                started: 900,
                ..pool_proc(CHROME)
            },
        );
        let (got, _) = run(&mut fake, &[10], point);
        assert_eq!(got, Ok(vec![1628]), "{point}");
        assert!(!fake.procs.contains_key(&1628), "{point}");
    }
}

#[test]
fn a_pid_reused_with_another_creation_time_isnt_taken_for_the_survivor_and_isnt_killed() {
    // The end sweeper started at 1000. 1628 survives its kill; by the next round it has gone and
    // a next session's process (created at 1500) has its pid. Known by pid alone it would be
    // the survivor, asked to end again and reported; by pid and creation time it is a new
    // process, spared by the cutoff.
    let mut fake = machine();
    fake.procs.insert(
        1628,
        Proc {
            started: 900,
            ..pool_proc(CHROME)
        },
    );
    fake.stays.insert(1628, u32::MAX);
    fake.procs.insert(13, pool_proc("stray.exe"));
    // The first round lists twice (the second after the opens); the next round's list finds
    // the reused pid.
    fake.appear.insert(
        3,
        vec![(
            1628,
            Proc {
                started: 1_500,
                ..pool_proc("next-agent.exe")
            },
        )],
    );
    let (got, _) = run(&mut fake, &[30], Point::Sweeper);
    assert_eq!(got, Ok(vec![13]));
    assert_eq!(
        fake.asked,
        [13, 1628],
        "the next session's process isn't asked to end"
    );
    assert_eq!(
        fake.procs.get(&1628).map(|p| p.image),
        Some("next-agent.exe")
    );
}
