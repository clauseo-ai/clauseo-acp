//! `ResetOwed`: a stop that kills without the set-up lock owes its users' resets there, without
//! the lock, read back and tried up to three times; the next stop that holds the lock folds them
//! into `ResetSids` before it writes `StopSequence`.

use super::*;
use crate::windows::setup::stop::owed::OWE_TRIES;

#[test]
fn a_mark_whose_lock_stays_held_is_owed_and_the_run_is_49_without_its_completion() {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.marks = Marks::LockBusy;
    fake.snapshots = vec![Ok(busy()), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.owed, [sid(1010), sid(1011)]);
    assert_eq!(fake.resets, Vec::<Sid>::new());
    // The reservation's and the mark's: the lock isn't tried again for the completion.
    assert_eq!(fake.locks, 2);
    assert_eq!(fake.values.get(STOP_SEQUENCE), None);
    let line = &fake.logged[0].1;
    assert!(line.contains("owed a reset in ResetOwed"), "{line}");
}

#[test]
fn an_owed_write_is_read_back_and_tried_three_times() {
    // Two writes that don't land: the third does.
    let mut fake = Fake::new(&[1010]);
    fake.marks = Marks::ReserveBusy;
    fake.snapshots = vec![Ok(vec![seen(10, Some(sid(1010)))]), Ok(Vec::new())];
    fake.owed_lost = 2;
    assert_eq!(stop(&mut fake), StopExit::NotReserved);
    assert_eq!(fake.owed, [sid(1010)]);
    let owes = fake.done.iter().filter(|line| *line == "owe 1").count();
    assert_eq!(owes, 3);
    // None land: the marks stop, 49.
    let mut fake = Fake::new(&[1010]);
    fake.marks = Marks::LockBusy;
    fake.snapshots = vec![Ok(vec![seen(10, Some(sid(1010)))]), Ok(Vec::new())];
    fake.owed_lost = OWE_TRIES;
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.owed, Vec::<Sid>::new());
    let line = &fake.logged[0].1;
    assert!(
        line.contains("ResetOwed wasn't written in 3 tries"),
        "{line}"
    );
    assert_eq!(OWE_TRIES, 3);
}

#[test]
fn the_next_stop_holding_the_lock_folds_the_owed_resets_before_its_sequence() {
    // The first stop couldn't reserve: it killed, owed 1010's reset, and wrote no sequence.
    let mut first = Fake::new(&[1010, 1011]);
    first.marks = Marks::ReserveBusy;
    first.snapshots = vec![Ok(vec![seen(10, Some(sid(1010)))]), Ok(Vec::new())];
    assert_eq!(stop(&mut first), StopExit::NotReserved);
    assert_eq!(first.completed(), None);
    // The next finds nothing running, and still folds 1010 in before its StopSequence.
    let mut next = Fake::new(&[1010, 1011]);
    next.owed = first.owed.clone();
    next.snapshots = vec![Ok(Vec::new())];
    assert_eq!(stop(&mut next), StopExit::Done);
    assert_eq!(next.resets, [sid(1010)]);
    assert_eq!(next.owed, Vec::<Sid>::new());
    let tail = [
        "mark 1",
        "clear owed",
        "set StopCompleted",
        "set StopSequence",
    ];
    let done: Vec<&str> = next.done.iter().map(String::as_str).collect();
    assert!(done.ends_with(&tail), "{done:?}");
    assert_eq!(next.completed().map(|(_, n)| n), Some(1));
}

#[test]
fn an_owed_list_that_cant_be_folded_writes_no_sequence() {
    let mut fake = Fake::new(&[1010]);
    fake.owed = vec![sid(1010)];
    fake.owed_clear_fails = Some("RegDeleteValueW failed (5)");
    fake.snapshots = vec![Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::NotMarked);
    assert_eq!(fake.completed(), None);
    assert_eq!(fake.owed, [sid(1010)], "still owed for the next stop");
    let line = &fake.logged[0].1;
    assert!(line.contains("ResetOwed wasn't cleared"), "{line}");
}
