//! The stop's reservation (step 3a) and what the completion checks before it writes: the number
//! is reserved from `StopNext` before the first snapshot, whatever the run then does, and the
//! completion writes nothing once the record is gone or holds another pool.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

/// The record's reads: the first at the start, the second at the reservation, the third at the
/// first mark (a run that found nothing has none) or else at the completion.
const AT_COMPLETION: usize = 2;
/// The reservation's read of the record.
const AT_RESERVATION: usize = 1;

#[test]
fn the_number_is_reserved_before_the_first_snapshot() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.done[..2], ["set StopNext", "snapshot"]);
    assert_eq!(
        fake.values.get(STOP_NEXT),
        Some(&2),
        "absent is 1, written 2"
    );
    assert!(!fake.held);
}

#[test]
fn success_writes_stop_completed_then_the_reserved_number() {
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(Vec::new())];
    fake.values.insert(STOP_NEXT, 5);
    fake.values.insert(STOP_SEQUENCE, 3);
    assert_eq!(stop(&mut fake), StopExit::Done);
    let tail = &fake.done[fake.done.len() - 2..];
    assert_eq!(tail, ["set StopCompleted", "set StopSequence"]);
    assert_eq!(
        fake.values.get(STOP_SEQUENCE),
        Some(&5),
        "n, not the old sequence and one"
    );
    assert_eq!(fake.values.get(STOP_NEXT), Some(&6));
}

#[test]
fn a_record_gone_or_changed_at_the_completion_writes_nothing() {
    let other = Pool {
        owner: Sid::parse(OWNER).unwrap(),
        users: vec![sid(1010), sid(1012)],
    };
    for (case, now) in [("gone", None), ("changed", Some(other))] {
        let mut fake = Fake::new(&[1010, 1011]);
        fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
        fake.pool_from = Some((AT_COMPLETION, Ok(now)));
        assert_eq!(stop(&mut fake), StopExit::NotMarked, "{case}");
        assert_eq!(fake.values.get(STOP_COMPLETED), None, "{case}");
        assert_eq!(fake.values.get(STOP_SEQUENCE), None, "{case}");
        assert_eq!(fake.done.last().map(String::as_str), Some("jobs"), "{case}");
        let (error, line) = &fake.logged[0];
        assert!(
            *error && line.contains("its pool changed"),
            "{case}: {line}"
        );
        assert!(!fake.held, "{case}");
    }
    // The same SIDs in another order are the same pool.
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(Vec::new())];
    let reordered = Pool {
        owner: Sid::parse(OWNER).unwrap(),
        users: vec![sid(1011), sid(1010)],
    };
    fake.pool_from = Some((AT_COMPLETION, Ok(Some(reordered))));
    assert_eq!(stop(&mut fake), StopExit::Done);
}

#[test]
fn a_reservation_that_fails_with_the_lock_taken_stops_nothing() {
    let nothing_done = |fake: &mut Fake, exit: StopExit, case: &str| {
        fake.snapshots = vec![Ok(busy())];
        fake.jobs = Ok(vec![Some(sid(1010))]);
        assert_eq!(stop(fake), exit, "{case}");
        assert!(fake.done.is_empty(), "{case}: {:?}", fake.done);
        assert!(fake.resets.is_empty(), "{case}");
        assert!(!fake.held, "{case}");
        let (error, line) = &fake.logged[0];
        assert!(
            *error && line.contains("nothing was stopped"),
            "{case}: {line}"
        );
    };
    let mut fake = Fake::new(&[1010]);
    fake.pool_from = Some((AT_RESERVATION, Ok(None)));
    nothing_done(&mut fake, StopExit::NotReserved, "the record is gone");
    let mut fake = Fake::new(&[1010]);
    fake.pool_from = Some((AT_RESERVATION, Err("RegGetValueW failed (5)".to_owned())));
    nothing_done(&mut fake, StopExit::NoRecord, "the record can't be read");
    let mut fake = Fake::new(&[1010]);
    fake.read_fails = Some(STOP_NEXT);
    nothing_done(&mut fake, StopExit::NoRecord, "StopNext can't be read");
    let mut fake = Fake::new(&[1010]);
    fake.values.insert(STOP_NEXT, u64::MAX);
    nothing_done(&mut fake, StopExit::NoRecord, "StopNext is at its end");
    assert_eq!(fake.values.get(STOP_NEXT), Some(&u64::MAX));
    assert_eq!(StopExit::NotReserved.as_code(), 50);
}

#[test]
fn a_run_that_ends_48_or_is_interrupted_still_uses_up_its_number() {
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(busy())];
    fake.refuse = vec![10, 16];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.values.get(STOP_NEXT), Some(&2));
    assert_eq!(fake.values.get(STOP_SEQUENCE), None);
    // Ended mid-pass: the number stays used, and the next run reserves the one after it.
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.end_at = Some(12);
    let ended = catch_unwind(AssertUnwindSafe(|| stop(&mut fake)));
    assert!(ended.is_err(), "the run was ended");
    assert_eq!(fake.values.get(STOP_NEXT), Some(&2));
    assert_eq!(fake.values.get(STOP_SEQUENCE), None);
    fake.end_at = None;
    fake.taken = 1;
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(
        fake.values.get(STOP_SEQUENCE),
        Some(&2),
        "1 was the ended run's"
    );
}
