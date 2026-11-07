//! The stop's completion (step 7): only a run that ends 0 counts `StopSequence` up and writes
//! `StopCompleted`, under the set-up lock; a run that ends 48 or 49, or one the task's end
//! interrupts, never does, and leaves an earlier pair as it is.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// A completion written by an earlier run, which a run that doesn't end 0 must leave alone.
const EARLIER: Option<(u64, u64)> = Some((7, 1));

#[test]
fn a_run_that_ends_0_writes_the_completion_last_and_under_the_lock() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.jobs = Ok(vec![Some(sid(1011))]);
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(
        fake.completed.map(|(_, sequence)| sequence),
        Some(1),
        "a first stop writes 1"
    );
    assert_eq!(fake.done.last().map(String::as_str), Some("completed"));
    assert!(!fake.held, "the lock is given back");
}

#[test]
fn each_completed_stop_counts_the_sequence_up_by_one() {
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.completed.map(|(_, sequence)| sequence), Some(1));
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.completed.map(|(_, sequence)| sequence), Some(2));
    // From an earlier record's sequence, one up.
    fake.completed = Some((7, 41));
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.completed.map(|(_, sequence)| sequence), Some(42));
}

#[test]
fn the_stamp_is_when_the_clean_pass_began_not_when_it_is_written() {
    let mut fake = Fake::new(&[1010, 1011]);
    // The first pass (at 0 s) stops three; the second begins at 1 s and finds none. The BITS
    // step begins at 2 s, and the completion is written then.
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    let (at, _) = fake.completed.unwrap();
    assert_eq!(
        at,
        filetime(Duration::from_secs(1)),
        "the clean pass's start"
    );
    assert_ne!(at, filetime(fake.now), "not the write's time");
    assert!(at < filetime(fake.now));
    // Nothing running: the one pass is the clean one, before the BITS step.
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(
        fake.completed.map(|(at, _)| at),
        Some(filetime(Duration::ZERO))
    );
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
