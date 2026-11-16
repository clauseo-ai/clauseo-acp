//! One live session per pool user (M1's interim rule): while a session of a matter is open, a
//! second launch of it is refused with `Assign::Busy`, the first keeps its user, and the user
//! serves the next launch once the open session's pass has run. Another matter never gets a user
//! with a live session; with every user so held, the pool is full as before.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::pool::{full, ledger_in, pool, reread, sids_in, user};
use super::*;
use crate::windows::pool::assign::Why;
use crate::windows::set_aside::SET_ASIDE_DIR;
use crate::windows::set_aside::tests::{
    exit_matter_on_test_volume, launch_beside_on_test_volume, launch_matter_on_test_volume,
};

fn tick(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000 + n)
}

#[test]
fn a_second_live_session_of_a_matter_is_busy_until_the_first_ends() {
    let data = TempDir::new("busy-data");
    let roots = TempDir::new("busy-matters");
    let records = TempDir::new("busy-ledger");
    let ledger = ledger_in(records.path(), &pool(4));
    let root = roots.dir("m/one");
    let one = matter();
    let mut sids = sids_in(&data);
    let open = OpenSessions::default();

    launch_matter_on_test_volume(&root, data.path(), &one, "s1", tick(1)).unwrap();
    let first = sids.assign(&one, "s1", &pool(4), reread(&ledger), &open);
    assert_eq!(first, Ok(user(1)));

    // s2 starts while s1 runs: the user is busy, and the matter keeps it.
    launch_beside_on_test_volume(&root, data.path(), &one, "s2", &["s1"], tick(2)).unwrap();
    let second = sids.assign(&one, "s2", &pool(4), reread(&ledger), &open);
    assert_eq!(second, Err(Assign::Busy(one)));
    assert_eq!(sids_in(&data).pool_user(&one), Some(&user(1)));
    // Asked again by s1 itself, still busy: s2 is open beside it.
    let again = sids.assign(&one, "s1", &pool(4), reread(&ledger), &open);
    assert_eq!(again, Err(Assign::Busy(one)));

    // s1's pass runs: its user is free again, for the launch still open.
    exit_matter_on_test_volume(&root, data.path(), &one, "s1", tick(3)).unwrap();
    let freed = sids.assign(&one, "s2", &pool(4), reread(&ledger), &open);
    assert_eq!(freed, Ok(user(1)), "kept, now free");
    exit_matter_on_test_volume(&root, data.path(), &one, "s2", tick(4)).unwrap();
    launch_matter_on_test_volume(&root, data.path(), &one, "s3", tick(5)).unwrap();
    let later = sids.assign(&one, "s3", &pool(4), reread(&ledger), &open);
    assert_eq!(later, Ok(user(1)));
}

#[test]
fn a_user_with_a_live_session_never_goes_to_another_matter() {
    let data = TempDir::new("busy-other-data");
    let roots = TempDir::new("busy-other-matters");
    let records = TempDir::new("busy-other-ledger");
    let ledger = ledger_in(records.path(), &pool(4));
    let (one, two) = (matter(), matter());
    let (root_one, root_two) = (roots.dir("m/one"), roots.dir("m/two"));
    let mut sids = sids_in(&data);
    launch_matter_on_test_volume(&root_one, data.path(), &one, "a1", tick(1)).unwrap();
    let mut open = sids.open_sessions().unwrap();
    assert_eq!(
        sids.assign(&one, "a1", &pool(1), reread(&ledger), &open),
        Ok(user(1))
    );

    // Every user busy: the pool is full, naming why, as it was before the rule.
    launch_matter_on_test_volume(&root_two, data.path(), &two, "b1", tick(2)).unwrap();
    open = sids.open_sessions().unwrap();
    let refused = sids.assign(&two, "b1", &pool(1), reread(&ledger), &open);
    assert_eq!(refused, Err(full(1, vec![(user(1), Why::Held(one))])));
    // With one more user in the pool (the set-up's top-up), the other matter gets that one.
    let topped = sids.assign(&two, "b1", &pool(2), reread(&ledger), &open);
    assert_eq!(topped, Ok(user(2)));
}

#[test]
fn an_unreadable_session_record_is_busy() {
    let data = TempDir::new("busy-unread-data");
    let roots = TempDir::new("busy-unread-matters");
    let records = TempDir::new("busy-unread-ledger");
    let ledger = ledger_in(records.path(), &pool(4));
    let root = roots.dir("m/one");
    let one = matter();
    let mut sids = sids_in(&data);
    let open = OpenSessions::default();
    launch_matter_on_test_volume(&root, data.path(), &one, "s1", tick(1)).unwrap();
    assert_eq!(
        sids.assign(&one, "s1", &pool(4), reread(&ledger), &open),
        Ok(user(1))
    );
    let record = format!("{SANDBOX_DIR}/{SET_ASIDE_DIR}/{one}/record.json");
    data.file(&record, "not a record");
    let got = sids.assign(&one, "s1", &pool(4), reread(&ledger), &open);
    assert_eq!(got, Err(Assign::Busy(one)));
}
