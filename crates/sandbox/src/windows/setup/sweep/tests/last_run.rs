//! The last run's record: `SweepLastExit` and then `SweepLastSequence` (the number it reserved),
//! written on every exit past the gate, before the log line, whatever the exit.

use super::*;
use crate::windows::setup::record_values::{
    SWEEP_LAST_EXIT, SWEEP_LAST_SEQUENCE, SWEEP_NEXT, SWEEP_SEQUENCE,
};

#[test]
fn a_clean_run_records_its_0_and_its_number() {
    let mut fake = Fake::new(&[1010]);
    fake.values.insert(SWEEP_NEXT, 7);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.last, [(SWEEP_LAST_EXIT, 0), (SWEEP_LAST_SEQUENCE, 7)]);
    assert_eq!(fake.values.get(SWEEP_SEQUENCE), Some(&7));
    assert!(!fake.logged[0].0, "{:?}", fake.logged);
}

#[test]
fn a_run_left_at_56_records_56_and_the_number_it_served_but_no_sequence() {
    let mut fake = Fake::new(&[1010]);
    fake.values.insert(SWEEP_NEXT, 4);
    fake.values.insert(SWEEP_SEQUENCE, 2);
    fake.unread("\\", "unread");
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.last, [(SWEEP_LAST_EXIT, 56), (SWEEP_LAST_SEQUENCE, 4)]);
    assert_eq!(
        fake.values.get(SWEEP_SEQUENCE),
        Some(&2),
        "still only a 0 run's"
    );
    assert_eq!(fake.values.get(SWEEP_NEXT), Some(&5));
    // A BITS job left: 57, with its number too.
    let mut jobs = Fake::new(&[1010]);
    jobs.jobs = Err("IBackgroundCopyManager::EnumJobs failed (0x80070005)".to_owned());
    assert_eq!(sweep(&mut jobs), SweepExit::JobsLeft);
    assert_eq!(jobs.last, [(SWEEP_LAST_EXIT, 57), (SWEEP_LAST_SEQUENCE, 1)]);
}

#[test]
fn a_run_that_reserved_no_number_records_its_exit_alone() {
    // The lock stayed held: swept with no number, and a task left wins over 58.
    let mut busy = Fake::new(&[1010]);
    busy.busy = true;
    busy.unread("\\", "unread");
    assert_eq!(sweep(&mut busy), SweepExit::TasksLeft);
    assert_eq!(busy.last, [(SWEEP_LAST_EXIT, 56)]);
    // The record gone at the reservation: nothing swept, 58.
    let mut gone = Fake::new(&[1010]);
    gone.gone_from = Some(1);
    assert_eq!(sweep(&mut gone), SweepExit::NotReserved);
    assert_eq!(gone.last, [(SWEEP_LAST_EXIT, 58)]);
}

#[test]
fn the_gates_refusals_and_an_empty_pool_record_nothing() {
    let mut not_system = Fake::new(&[1010]);
    not_system.system = false;
    assert_eq!(sweep(&mut not_system), SweepExit::NotAllowed);
    let mut not_pool = Fake::new(&[1010]);
    not_pool.not_pool = true;
    assert_eq!(sweep(&mut not_pool), SweepExit::NotPool);
    let mut none = Fake::new(&[1010]);
    none.pool = Ok(None);
    assert_eq!(sweep(&mut none), SweepExit::Done);
    for fake in [not_system, not_pool, none] {
        assert!(fake.last.is_empty(), "{:?}", fake.last);
    }
}

#[test]
fn a_last_run_that_cant_be_recorded_keeps_the_exit_and_says_so() {
    let mut fake = Fake::new(&[1010]);
    fake.write_fails = Some(SWEEP_LAST_EXIT);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert!(fake.last.is_empty(), "no number without its exit");
    let (error, line) = &fake.logged[0];
    assert!(*error, "{line}");
    assert!(
        line.contains("its exit wasn't recorded: SweepLastExit wasn't written"),
        "{line}"
    );
    let mut second = Fake::new(&[1010]);
    second.write_fails = Some(SWEEP_LAST_SEQUENCE);
    assert_eq!(sweep(&mut second), SweepExit::Done);
    assert_eq!(second.last, [(SWEEP_LAST_EXIT, 0)]);
    assert!(
        second.logged[0]
            .1
            .contains("SweepLastSequence wasn't written")
    );
}
