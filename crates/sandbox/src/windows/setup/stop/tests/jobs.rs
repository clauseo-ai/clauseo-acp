//! The stop's BITS step (`cancel_jobs`): only the pool users' jobs are cancelled, whatever the
//! process passes found, their owners are marked for a reset, and a job that isn't cancelled, or
//! a list that can't be read, is 48.

use super::*;

/// A stop of pool users 1010 and 1011, with no process running, over BITS jobs owned by
/// `owners`, whose cancel fails at `refused`.
fn run(owners: Vec<Option<Sid>>, refused: &[usize]) -> (StopExit, Fake) {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(vec![seen(11, Some(Sid::parse(OWNER).unwrap()))])];
    fake.jobs = Ok(owners);
    fake.jobs_refuse = refused.to_vec();
    (stop(&mut fake), fake)
}

/// The job indexes `done` cancelled, in order.
fn cancelled(fake: &Fake) -> Vec<usize> {
    let at = fake
        .done
        .iter()
        .filter_map(|line| line.strip_prefix("cancel "));
    at.map(|at| at.parse().unwrap()).collect()
}

#[test]
fn only_the_pool_users_jobs_are_cancelled_even_with_no_process_running() {
    let owner = Sid::parse(OWNER).unwrap();
    let system = Sid::parse("S-1-5-18").unwrap();
    // The owner's, SYSTEM's and another local account's jobs stay; 1010's and 1011's go.
    let owners = vec![
        Some(owner),
        Some(sid(1010)),
        Some(system),
        Some(sid(1013)),
        Some(sid(1011)),
        Some(sid(1010)),
    ];
    let (exit, fake) = run(owners, &[]);
    assert_eq!(exit, StopExit::Done);
    assert_eq!(cancelled(&fake), [1, 4, 5]);
    assert_eq!(fake.stopped(), Vec::<u32>::new());
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    let line = &fake.logged[0].1;
    assert!(line.contains("0 processes stopped"), "{line}");
    assert!(line.contains("3 BITS jobs cancelled, 0 not"), "{line}");
    assert!(line.contains("2 users marked"), "{line}");
}

#[test]
fn a_failed_cancel_is_48_and_its_owner_still_marked() {
    let (exit, fake) = run(vec![Some(sid(1013)), Some(sid(1011))], &[1]);
    assert_eq!(exit, StopExit::NotStopped);
    assert_eq!(cancelled(&fake), Vec::<usize>::new());
    assert_eq!(fake.resets, [sid(1011)]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("0 BITS jobs cancelled, 1 not"), "{line}");
    assert!(line.contains("wasn't cancelled"), "{line}");
}

#[test]
fn a_job_whose_owner_cant_be_read_is_left_and_is_48() {
    let (exit, fake) = run(vec![None, Some(sid(1010))], &[]);
    assert_eq!(exit, StopExit::NotStopped);
    assert_eq!(cancelled(&fake), [1]);
    assert_eq!(fake.resets, [sid(1010)]);
}

#[test]
fn a_list_that_cant_be_read_is_48_after_the_processes_are_stopped() {
    let mut fake = Fake::new(&[1010]);
    fake.snapshots = vec![Ok(vec![seen(10, Some(sid(1010)))]), Ok(Vec::new())];
    fake.jobs = Err("CoCreateInstance(BackgroundCopyManager) failed (0x80070422)".to_owned());
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(fake.stopped(), [10]);
    assert_eq!(fake.resets, [sid(1010)]);
    let line = &fake.logged[0].1;
    assert!(line.contains("BITS jobs couldn't be listed"), "{line}");
}
