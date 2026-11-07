//! How the stop reads each process's user ([`pick`]): the kernel's process list first, whatever
//! the token's DACL says, the token as a cross-check, the process object's owner when neither
//! names one; and a process whose user no source gives counts as unknown, never passed over.

use super::*;

/// A process opened to be stopped, its user as each source gave it.
fn process(pid: u32, listed: Option<Sid>, token: Option<Sid>, owner: Option<Sid>) -> Process {
    Process {
        pid,
        listed,
        token,
        owner,
        can_stop: true,
    }
}

/// A stop over one snapshot holding `processes`, then an empty one.
fn run(processes: Vec<Process>) -> (StopExit, Fake) {
    let mut fake = Fake::new(&[1010, 1011]);
    fake.snapshots = vec![Ok(processes), Ok(Vec::new())];
    (stop(&mut fake), fake)
}

#[test]
fn a_token_shut_to_system_is_read_from_the_process_list() {
    // The pool user's token denies SYSTEM, so it can't be read: the list still names its user.
    let (exit, fake) = run(vec![process(30, Some(sid(1010)), None, None)]);
    assert_eq!(exit, StopExit::Done);
    assert_eq!(fake.stopped(), [30]);
    assert_eq!(fake.resets, [sid(1010)]);
}

#[test]
fn with_no_list_entry_the_token_then_the_owner_say_whose_it_is() {
    let pool_token = process(31, None, Some(sid(1010)), None);
    let pool_owner = process(32, None, None, Some(sid(1011)));
    // Another account owns 33: neither a pool user's nor unknown.
    let other_owner = process(33, None, None, Some(Sid::parse(OWNER).unwrap()));
    let (exit, fake) = run(vec![pool_token, pool_owner, other_owner]);
    assert_eq!(exit, StopExit::Done);
    assert_eq!(fake.stopped(), [31, 32]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
}

#[test]
fn a_process_no_source_names_is_unknown_and_never_done() {
    let unknown = process(34, None, None, None);
    let mut fake = Fake::new(&[1010, 1011]);
    let first = vec![
        unknown.clone(),
        process(10, Some(sid(1010)), Some(sid(1010)), None),
    ];
    // The last snapshot, which finds no pool user's process, still holds it.
    fake.snapshots = vec![Ok(first), Ok(vec![unknown])];
    let exit = stop(&mut fake);
    assert_eq!(exit, StopExit::NotStopped);
    // It isn't stopped, but the pool user's process beside it is, and its user marked.
    assert_eq!(fake.stopped(), [10]);
    assert_eq!(fake.resets, [sid(1010)]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("1 unknown"), "{line}");
    assert!(line.contains("user couldn't be read"), "{line}");
}

#[test]
fn a_list_and_token_that_disagree_are_unknown() {
    let (exit, fake) = run(vec![process(35, Some(sid(1010)), Some(sid(1013)), None)]);
    assert_eq!(exit, StopExit::NotStopped);
    assert_eq!(fake.stopped(), Vec::<u32>::new());
    assert!(fake.logged[0].1.contains("1 unknown"), "{:?}", fake.logged);
}

#[test]
fn only_a_protected_process_and_the_kernels_are_exempt_from_unknown() {
    let protected = Process {
        can_stop: false,
        ..process(36, None, None, None)
    };
    let unknown_kernel = [0, 4, OWN_PID].map(|pid| process(pid, None, None, None));
    let mut seen = vec![protected];
    seen.extend(unknown_kernel);
    let pool = [sid(1010)];
    assert_eq!(pick(&seen, &pool, OWN_PID), Picked::default());
    let (exit, fake) = run(seen);
    assert_eq!(exit, StopExit::Done);
    assert!(fake.logged[0].1.contains("0 unknown"), "{:?}", fake.logged);
}

#[test]
fn pick_takes_each_pid_once_compares_sids_exactly_and_never_takes_this_process() {
    let pool = [sid(1010)];
    let seen = [
        process(40, Some(sid(1010)), None, None),
        process(40, Some(sid(1010)), None, None),
        // A digit more, and the same RID in another domain.
        process(41, Some(sid(10100)), None, None),
        process(42, Sid::parse("S-1-5-21-1-2-4-1010").ok(), None, None),
        process(OWN_PID, Some(sid(1010)), None, None),
        process(4, Some(sid(1010)), None, None),
    ];
    let picked = pick(&seen, &pool, OWN_PID);
    assert_eq!(picked.stop, [(40, sid(1010))]);
    assert_eq!(picked.unknown, 0);
}

#[test]
fn a_pool_process_with_no_handle_keeps_the_passes_going() {
    // The list names 37 but the snapshot couldn't open it (started since): it can't be
    // terminated this pass, and the next pass finds it gone.
    let mut fake = Fake::new(&[1010]);
    let unopened = Process {
        can_stop: false,
        ..process(37, Some(sid(1010)), None, None)
    };
    fake.refuse = vec![37];
    fake.snapshots = vec![Ok(vec![unopened]), Ok(Vec::new())];
    assert_eq!(stop(&mut fake), StopExit::Done);
    assert_eq!(fake.snapshots_taken(), 2);
    assert_eq!(fake.resets, [sid(1010)]);
    assert!(
        fake.logged[0].1.contains("(1 retried)"),
        "{:?}",
        fake.logged
    );
}
