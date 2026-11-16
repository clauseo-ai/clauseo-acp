//! A process whose kill is refused but that exits by itself within the window (a console host
//! once its client is killed) is no failure; one still running after it fails the first act
//! closed and is logged past it. The window is counted in the fake's pauses.

use super::*;
use crate::windows::runner::sweep::refused::{EXIT_POLL, EXIT_WAIT};

/// The window's pauses: one look, then one after each.
fn window() -> u32 {
    u32::try_from(EXIT_WAIT.as_millis() / EXIT_POLL.as_millis()).unwrap()
}

/// A probe (12) and its console host (13), whose kill is always refused.
fn console_machine() -> Fake {
    let mut fake = machine();
    fake.procs.insert(12, pool_proc("probe.exe"));
    fake.procs.insert(13, pool_proc("conhost.exe"));
    fake.deny_kill.insert(13, u32::MAX);
    fake
}

#[test]
fn a_refused_process_that_exits_within_the_window_passes() {
    for after in [1, 3, window()] {
        let mut fake = console_machine();
        fake.exits_after.insert(13, after);
        let (got, lines) = run(&mut fake, &[], Point::Start);
        assert_eq!(got, Ok(vec![12, 13]), "after {after}");
        assert_eq!(fake.killed, [12], "after {after}");
        let pauses = usize::try_from(after).unwrap();
        assert_eq!(fake.pauses, vec![EXIT_POLL; pauses], "after {after}");
        assert_eq!(
            lines.last().map(String::as_str),
            Some(
                "clauseo-sandbox-run: sweep (before launch): the sandbox user's process 13 \
                 (conhost.exe) exited by itself after its kill was refused (5)"
            ),
            "after {after}"
        );
    }
}

#[test]
fn a_refused_process_still_running_after_the_window_fails_the_first_act_closed() {
    for after in [None, Some(window() + 1)] {
        let mut fake = console_machine();
        if let Some(after) = after {
            fake.exits_after.insert(13, after);
        }
        let got = run(&mut fake, &[], Point::Start).0;
        assert!(
            matches!(&got, Err(SweepError::Kill { pid: 13, code, .. }) if code == "5"),
            "{after:?}: {got:?}"
        );
        // Looked at for the whole window, every poll, and no longer.
        let polls = usize::try_from(window()).unwrap();
        assert_eq!(fake.pauses, vec![EXIT_POLL; polls], "{after:?}");
        assert_eq!(fake.killed, [12], "{after:?}");
    }
}

#[test]
fn past_the_first_act_a_refused_process_is_logged_only_when_it_stays() {
    for point in [Point::Tick, Point::End] {
        let mut fake = console_machine();
        fake.exits_after.insert(13, 2);
        let (got, lines) = run(&mut fake, &[], point);
        assert_eq!(got, Ok(vec![12, 13]), "{point}");
        assert!(
            !lines.iter().any(|l| l.contains("can't be ended")),
            "{lines:?}"
        );
        let mut fake = console_machine();
        let (got, lines) = run(&mut fake, &[], point);
        let left = SweepError::Left {
            count: 1,
            unkillable: 1,
            images: vec!["conhost.exe".to_owned()],
        };
        assert_eq!(got, Err(left), "{point}");
        let failures: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("can't be ended"))
            .collect();
        assert_eq!(
            failures,
            [&format!(
                "clauseo-sandbox-run: sweep ({point}): the sandbox user's process 13 \
                 (conhost.exe) can't be ended (5)"
            )],
            "{point}"
        );
    }
}
