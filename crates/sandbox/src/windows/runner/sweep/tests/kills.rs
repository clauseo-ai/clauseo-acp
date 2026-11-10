//! A kill is its handle signalled: a kill whose call succeeds but leaves the process running is
//! a survivor (an escapee, which the inner reports), never "killed"; an exited process still
//! listed is no kill and can't keep the rounds going; a process is known by its pid and creation
//! time; and a process in a job of its own is found by its user all the same.

use super::*;

const CHROME: &str = "chrome-headless-shell.exe";

fn killed_lines(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.contains(") killed ")).collect()
}

#[test]
fn a_kill_that_succeeds_but_leaves_the_process_running_is_a_survivor_and_an_escapee() {
    for point in [Point::Tick, Point::End] {
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
        // An escapee: the inner reports it, and the session's marker awaits the SYSTEM stop.
        assert_eq!(
            got.unwrap_err().escapees(),
            Some(Escapees {
                count: 1,
                images: vec![CHROME.to_owned()],
            })
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
    for point in [Point::Tick, Point::End] {
        let mut fake = machine();
        fake.ends = Ends::Listed;
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
fn a_sweep_passes_over_a_wer_clone_that_exited_but_is_still_listed() {
    // The VM's process: WER's zero-thread clone of the crashed main, outside the job, which the
    // main holds open until the job goes. Its handle is signalled: the sweep leaves it, logs it
    // once, and passes rather than ending in "still started processes after 8 rounds".
    let mut fake = machine();
    fake.procs.insert(10, pool_proc("inner.exe"));
    fake.procs.insert(1628, pool_proc(CHROME));
    fake.dead.insert(1628);
    let (got, lines) = run(&mut fake, &[10], Point::End);
    assert_eq!(got, Ok(vec![]));
    assert_eq!(fake.asked, Vec::<u32>::new());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("had already exited"), "{lines:?}");
}

#[test]
fn a_process_in_a_job_of_its_own_is_found_by_its_user_and_killed() {
    // Chrome's sandboxed child: its own job, which the inner's job's pid list doesn't name; its
    // DACL shuts its user out, so only the service's list names its user.
    for point in [Point::Tick, Point::End] {
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
    // The inner started at 1000. 1628 survives its kill; by the next round it has ended and a
    // process of the job (created at 1500) has its pid. Known by pid alone it would be the
    // survivor; by pid and creation time it is a new process, kept as the job's. The
    // survivor's own handle, asked again, is signalled: it is killed.
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
                in_job: true,
                ..pool_proc("agent-child.exe")
            },
        )],
    );
    let (got, _) = run(&mut fake, &[10], Point::Tick);
    assert_eq!(got, Ok(vec![13, 1628]));
    // The second ask is the survivor's handle's, which the job's process isn't.
    assert_eq!(fake.asked, [13, 1628, 1628]);
    assert_eq!(fake.killed, [13]);
    assert_eq!(
        fake.procs.get(&1628).map(|p| p.image),
        Some("agent-child.exe")
    );
}

#[test]
fn a_survivor_that_drops_out_of_the_lists_is_still_a_survivor() {
    // The verdict is the held handle's: once asked to end, 1628 is in neither list, yet still
    // runs. Each round's end looks at its handle, listed or not.
    for spawns in [false, true] {
        let mut fake = machine();
        fake.procs.insert(1628, pool_proc(CHROME));
        fake.stays.insert(1628, u32::MAX);
        fake.vanish.insert(1628);
        if spawns {
            // A second round, which lists no 1628.
            fake.procs.insert(13, pool_proc("stray.exe"));
            fake.spawns.insert(13, (40, pool_proc("child.exe")));
        }
        let (got, _) = run(&mut fake, &[], Point::Tick);
        assert_eq!(
            got,
            Err(SweepError::Left {
                count: 1,
                unkillable: 1,
                images: vec![CHROME.to_owned()],
            }),
            "{spawns}"
        );
        assert!(fake.procs.contains_key(&1628));
    }
}

#[test]
fn one_deadline_caps_every_wait_of_a_sweep() {
    // 15 survivors: the first 12 kills wait KILL_WAIT each, the deadline's whole 60 s; the rest
    // wait for nothing, and all 15 are survivors.
    let mut fake = machine();
    for pid in 100..115 {
        fake.procs.insert(pid, pool_proc(CHROME));
        fake.stays.insert(pid, u32::MAX);
    }
    let (got, _) = run(&mut fake, &[], Point::Tick);
    assert!(
        matches!(
            got,
            Err(SweepError::Left {
                count: 15,
                unkillable: 15,
                ..
            })
        ),
        "{got:?}"
    );
    let full = usize::try_from(SWEEP_DEADLINE.as_secs() / KILL_WAIT.as_secs()).unwrap();
    assert_eq!(fake.waits, vec![KILL_WAIT; full]);
    assert_eq!(fake.clock, SWEEP_DEADLINE);
}

#[test]
fn a_sweep_whose_kills_keep_it_going_past_its_deadline_stops_there() {
    // A process that starts another each time it is killed, beside five strays, each kill taking
    // KILL_WAIT: the first round's six kills take 30 s and each later round's one 5 s, so the
    // eighth round would begin at the deadline, and doesn't.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("hydra.exe"));
    for pid in 100..105 {
        fake.procs.insert(pid, pool_proc("stray.exe"));
    }
    fake.forever = Some(12);
    fake.ends = Ends::Slowly;
    let (got, _) = run(&mut fake, &[], Point::Tick);
    assert_eq!(got, Err(SweepError::Deadline));
    assert_eq!(fake.clock, SWEEP_DEADLINE);
    assert_eq!(fake.killed.len(), 12);
    // A tree that outran the sweep: an escapee, so the marker awaits the SYSTEM stop.
    assert_eq!(got.unwrap_err().escapees().map(|e| e.count), Some(0));
}

#[test]
fn a_process_that_couldnt_be_opened_or_ended_counts_however_it_is_listed() {
    // 12 can't be opened, and the next round lists it with no owner (its DACL shut both lists
    // out): with no handle to say it ended, it is still counted. 13's kill is refused and it then
    // drops out of the lists while still running: its held handle says so. 14's kill lets 40
    // appear, for a second round.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("shut.exe"));
    fake.deny_open.insert(12);
    fake.unnamed_at.insert(3, vec![12]);
    fake.procs.insert(13, pool_proc("refusing.exe"));
    fake.deny_kill.insert(13, u32::MAX);
    fake.vanish.insert(13);
    fake.procs.insert(14, pool_proc("stray.exe"));
    fake.spawns.insert(14, (40, pool_proc("child.exe")));
    let (got, _) = run(&mut fake, &[], Point::Tick);
    assert_eq!(
        got,
        Err(SweepError::Left {
            count: 2,
            unkillable: 2,
            images: vec!["refusing.exe".to_owned(), "shut.exe".to_owned()],
        })
    );
    assert_eq!(fake.killed, [14, 40]);
    assert!(fake.procs.contains_key(&13));
}

#[test]
fn a_wait_is_whole_milliseconds_and_never_infinite() {
    assert_eq!(wait_millis(Duration::ZERO), 0);
    assert_eq!(wait_millis(KILL_WAIT), 5_000);
    assert_eq!(wait_millis(Duration::from_micros(1_999)), 1);
    // INFINITE is u32::MAX: a wait past it, or exactly it, saturates one below.
    let longest = u32::MAX - 1;
    assert_eq!(
        wait_millis(Duration::from_millis(u64::from(u32::MAX))),
        longest
    );
    assert_eq!(wait_millis(Duration::MAX), longest);
    assert_eq!(wait_millis(SWEEP_DEADLINE * 1_000_000), longest);
}

#[test]
fn a_process_that_couldnt_be_opened_stops_counting_once_no_list_has_its_pid() {
    // 12 can't be opened, then exits by itself: the next round's listing has no pid 12 at all,
    // so it no longer counts, and the sweep passes. 14's kill lets 40 appear, for that round.
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("shut.exe"));
    fake.deny_open.insert(12);
    fake.gone_at.insert(3, vec![12]);
    fake.procs.insert(14, pool_proc("stray.exe"));
    fake.spawns.insert(14, (40, pool_proc("child.exe")));
    let (got, lines) = run(&mut fake, &[], Point::Tick);
    assert_eq!(got, Ok(vec![14, 40]));
    // Its failure was logged once, in the round that met it.
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("(shut.exe) can't be opened"))
            .count(),
        1,
        "{lines:?}"
    );
}
