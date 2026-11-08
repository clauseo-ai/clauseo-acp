//! A stop that can't take the set-up lock still stops the pool's processes and cancels their
//! jobs, writing nothing to the record but `ResetOwed`; and each mark checks the record still
//! holds the pool.

use super::*;

/// The record's reads: the gate's, the reservation's, then the first mark's.
const AT_FIRST_MARK: usize = 2;

#[test]
fn a_held_lock_still_stops_everything_and_writes_only_the_owed_resets() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.marks = Marks::ReserveBusy;
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    fake.jobs = Ok(vec![Some(sid(1010)), Some(sid(1013))]);
    assert_eq!(stop(&mut fake), StopExit::NotReserved);
    assert_eq!(StopExit::NotReserved.as_code(), 50);
    // Every pool user's process stopped, and its job cancelled; the lawyer's and others' left.
    assert_eq!(fake.stopped(), [10, 12, 16]);
    assert!(
        fake.done.contains(&"cancel 0".to_owned()),
        "{:?}",
        fake.done
    );
    assert!(
        !fake.done.contains(&"cancel 1".to_owned()),
        "{:?}",
        fake.done
    );
    // Nothing written under the lock: no StopNext, no marks, no completion; the lock tried once,
    // so its wait came once. Every pool user found is owed a reset, without the lock.
    assert!(fake.values.is_empty(), "{:?}", fake.values);
    assert_eq!(fake.resets, Vec::<Sid>::new());
    assert_eq!(fake.owed, [sid(1010), sid(1011)]);
    assert_eq!(fake.locks, 1);
    let (error, line) = &fake.logged[0];
    assert!(*error, "{line}");
    assert!(
        line.contains("3 processes stopped")
            && line.contains(
                "nothing but ResetOwed was written to the record: StopNext wasn't reserved"
            ),
        "{line}"
    );
}

#[test]
fn a_held_lock_with_a_process_left_is_48() {
    let mut fake = Fake::new(&[1010]);
    fake.marks = Marks::ReserveBusy;
    fake.snapshots = vec![Ok(busy())];
    fake.refuse = vec![10];
    fake.hang = vec![16];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.values, BTreeMap::new());
}

#[test]
fn a_mark_checks_the_record_still_holds_the_pool() {
    let changed = Pool {
        owner: Sid::parse(OWNER).unwrap(),
        users: vec![sid(1010), sid(1012)],
    };
    for (case, now) in [("gone", Ok(None)), ("changed", Ok(Some(changed)))] {
        let mut fake = Fake::new(&[1010, 1011]);
        fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
        fake.pool_from = Some((AT_FIRST_MARK, now));
        // The processes are still stopped, but no user is marked, and the run isn't 0.
        assert_eq!(stop(&mut fake), StopExit::NotMarked, "{case}");
        assert_eq!(fake.resets, Vec::<Sid>::new(), "{case}");
        assert_eq!(fake.stopped(), [10, 12, 16], "{case}");
        assert_eq!(fake.values.get(STOP_SEQUENCE), None, "{case}");
        let (error, line) = &fake.logged[0];
        assert!(
            *error && line.contains("its pool changed"),
            "{case}: {line}"
        );
    }
    // The same SIDs in another order are the same pool: marked.
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    let reordered = Pool {
        owner: Sid::parse(OWNER).unwrap(),
        users: vec![sid(1011), sid(1010)],
    };
    fake.pool_from = Some((AT_FIRST_MARK, Ok(Some(reordered))));
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.resets.len(), 2);
}
