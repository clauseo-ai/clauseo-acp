//! The stop's completion (step 7): only a run that ends 0 writes `StopCompleted` and
//! `StopLeft` 0, under the set-up lock; a run that ends 48 or 49, or one the task's end
//! interrupts, never does, and leaves an earlier one as it is.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// A completion written by an earlier run, which a run that doesn't end 0 must leave alone.
const EARLIER: Option<(u32, usize)> = Some((0, 1));

#[test]
fn a_run_that_ends_0_writes_the_completion_with_none_left_last_and_under_the_lock() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.jobs = Ok(vec![Some(sid(1011))]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.completed, Some((0, 1)), "StopLeft 0, written once");
    assert_eq!(fake.done.last().map(String::as_str), Some("completed"));
    assert!(!fake.held, "the lock is given back");
}

#[test]
fn a_run_that_ends_48_writes_no_completion() {
    // A pool user's process that won't end, past the passes' time.
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(busy())];
    fake.refuse = vec![10, 16];
    fake.completed = EARLIER;
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.completed, EARLIER);
    // A process whose user can't be read.
    let mut fake = Fake::new(&[1010]);
    let unread = Process {
        pid: 30,
        can_stop: true,
        ..Process::default()
    };
    fake.snapshots = vec![Ok(vec![unread])];
    fake.completed = EARLIER;
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.completed, EARLIER);
    // A pool user's BITS job that won't cancel.
    let mut fake = Fake::new(&[1010]);
    fake.jobs = Ok(vec![Some(sid(1010))]);
    fake.jobs_refuse = vec![0];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.completed, None);
    assert!(
        !fake.done.contains(&"completed".to_owned()),
        "{:?}",
        fake.done
    );
}

#[test]
fn a_run_that_ends_49_writes_no_completion() {
    // The marks fail: nothing is written after.
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.marks = Marks::WriteFails;
    fake.completed = EARLIER;
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.completed, EARLIER);
    // The completion's own write fails: 49 too, never 0.
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.marks = Marks::CompletionFails;
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.completed, None);
    assert!(!fake.held);
    let (error, line) = &fake.logged[0];
    assert!(
        *error && line.contains("StopCompleted wasn't written"),
        "{line}"
    );
}

#[test]
fn a_run_the_tasks_end_interrupts_writes_no_completion_but_keeps_its_marks() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.completed = EARLIER;
    // Ended mid-pass, at the second termination: nothing after it runs.
    fake.end_at = Some(12);
    let ended = catch_unwind(AssertUnwindSafe(|| stop(&mut fake)));
    assert!(ended.is_err(), "the run was ended");
    assert_eq!(fake.completed, EARLIER);
    assert!(
        !fake.done.contains(&"completed".to_owned()),
        "{:?}",
        fake.done
    );
    // The users it found were marked before anything was stopped.
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert_eq!(fake.stopped(), [10]);
}
