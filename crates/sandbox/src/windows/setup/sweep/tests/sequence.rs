//! The sweep's sequence: `SweepNext` reserved before the walk, and `SweepCompleted` then
//! `SweepSequence = n` written only by a run that ends 0, as the stop's are.

use super::record::NOW;
use super::*;
use crate::windows::setup::record_values::{SWEEP_COMPLETED, SWEEP_NEXT, SWEEP_SEQUENCE};

#[test]
fn the_number_is_reserved_before_the_walk_and_written_once_the_run_is_clean() {
    let mut fake = Fake::new(&[1010]);
    fake.task("\\", "pool", user("S-1-5-21-1-2-3-1010"));
    fake.values.insert(SWEEP_NEXT, 7);
    fake.values.insert(SWEEP_SEQUENCE, 5);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.done[..3], ["lock", "set SweepNext", "root"]);
    let tail = &fake.done[fake.done.len() - 3..];
    assert_eq!(tail, ["lock", "set SweepCompleted", "set SweepSequence"]);
    assert_eq!(fake.values.get(SWEEP_NEXT), Some(&8));
    assert_eq!(
        fake.values.get(SWEEP_SEQUENCE),
        Some(&7),
        "n, not the old one and one"
    );
    assert_eq!(fake.values.get(SWEEP_COMPLETED), Some(&NOW));
    assert_eq!(fake.held, 0);
}

#[test]
fn an_absent_sweep_next_is_1() {
    let mut fake = Fake::new(&[1010]);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.values.get(SWEEP_SEQUENCE), Some(&1));
    assert_eq!(fake.values.get(SWEEP_NEXT), Some(&2));
}

#[test]
fn a_run_left_unclean_keeps_its_number_but_writes_no_sequence() {
    let mut fake = Fake::new(&[1010]);
    fake.unread("\\", "unread");
    fake.values.insert(SWEEP_SEQUENCE, 3);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.values.get(SWEEP_NEXT), Some(&2));
    assert_eq!(fake.values.get(SWEEP_SEQUENCE), Some(&3));
    assert_eq!(fake.values.get(SWEEP_COMPLETED), None);
    let mut jobs = Fake::new(&[1010]);
    jobs.jobs = Err("IBackgroundCopyManager::EnumJobs failed (0x80070005)".to_owned());
    assert_eq!(sweep(&mut jobs), SweepExit::JobsLeft);
    assert_eq!(jobs.values.get(SWEEP_SEQUENCE), None);
}

#[test]
fn a_held_lock_still_sweeps_but_records_nothing() {
    let mut fake = Fake::new(&[1010]);
    fake.busy = true;
    fake.task("\\", "pool", user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::NotReserved);
    assert_eq!(SweepExit::NotReserved.as_code(), 58);
    assert_eq!(fake.deleted(), ["\\pool"]);
    assert!(fake.values.is_empty(), "{:?}", fake.values);
    let (error, line) = &fake.logged[0];
    assert!(*error);
    assert!(
        line.contains("nothing was recorded: SweepNext wasn't reserved: the set-up lock"),
        "{line}"
    );
    // A task left wins over it.
    let mut left = Fake::new(&[1010]);
    left.busy = true;
    left.unread("\\", "unread");
    assert_eq!(sweep(&mut left), SweepExit::TasksLeft);
}

#[test]
fn a_record_gone_at_the_reservation_touches_nothing() {
    let mut fake = Fake::new(&[1010]);
    // The gate's read is the first; the reservation's the second.
    fake.gone_from = Some(1);
    fake.task("\\", "pool", user("S-1-5-21-1-2-3-1010"));
    assert_eq!(sweep(&mut fake), SweepExit::NotReserved);
    assert_eq!(fake.done, ["lock"]);
    assert!(fake.logged[0].1.contains("nothing was touched: SweepNext"));
    let mut failing = Fake::new(&[1010]);
    failing.write_fails = Some(SWEEP_NEXT);
    assert_eq!(sweep(&mut failing), SweepExit::NotReserved);
    assert_eq!(failing.done, ["lock"]);
}

#[test]
fn a_completion_that_cant_be_written_is_58() {
    for case in ["gone", "write", "past"] {
        let mut fake = Fake::new(&[1010]);
        match case {
            // The gate's read, the reservation's, then the completion's.
            "gone" => fake.gone_from = Some(2),
            "write" => fake.write_fails = Some(SWEEP_COMPLETED),
            _ => {
                fake.values.insert(SWEEP_SEQUENCE, 9);
            }
        }
        assert_eq!(sweep(&mut fake), SweepExit::NotRecorded, "{case}");
        assert_ne!(fake.values.get(SWEEP_SEQUENCE), Some(&1), "{case}");
        let (error, line) = &fake.logged[0];
        assert!(
            *error && line.contains("SweepCompleted wasn't written"),
            "{case}: {line}"
        );
    }
    assert_eq!(SweepExit::NotRecorded.as_code(), 59);
}
