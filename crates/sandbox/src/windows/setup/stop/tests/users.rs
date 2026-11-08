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
        session: Some(1),
        ..Process::default()
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
fn with_no_list_entry_the_token_says_whose_it_is_and_a_pool_owner_adds_it() {
    let pool_token = process(31, None, Some(sid(1010)), None);
    let pool_owner = process(32, None, None, Some(sid(1011)));
    let (exit, fake) = run(vec![pool_token, pool_owner]);
    assert_eq!(exit, StopExit::Done);
    assert_eq!(fake.stopped(), [31, 32]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
}

#[test]
fn an_owner_that_isnt_a_pool_user_never_clears_a_process() {
    // The owner is the creator, not the user: Administrators owns an elevated administrator's
    // processes, whoever runs them. With no user from the list or the token, 33 is unknown.
    for owner in [OWNER, "S-1-5-32-544"] {
        let other_owner = process(33, None, None, Some(Sid::parse(owner).unwrap()));
        let (exit, fake) = run(vec![other_owner]);
        assert_eq!(exit, StopExit::NotStopped, "{owner}");
        assert_eq!(fake.stopped(), Vec::<u32>::new(), "{owner}");
        assert!(fake.logged[0].1.contains("1 unknown"), "{:?}", fake.logged);
    }
}

#[test]
fn a_failed_process_list_leaves_only_the_token_or_a_pool_owner_to_name_a_process() {
    let failed = |process: Process| Process {
        list_failed: true,
        ..process
    };
    // The token names 50 and a pool owner names 51; both are stopped, and that is all.
    let named = vec![
        failed(process(50, None, Some(sid(1010)), None)),
        failed(process(51, None, None, Some(sid(1011)))),
        failed(process(52, None, Some(Sid::parse(OWNER).unwrap()), None)),
    ];
    let (exit, fake) = run(named);
    assert_eq!(exit, StopExit::Done);
    assert_eq!(fake.stopped(), [50, 51]);
    // A token that can't be read is unknown, opened or not: with no list, a protected process
    // is no longer exempt, since nothing would name a pool user's.
    for can_stop in [true, false] {
        let unread = Process {
            can_stop,
            ..failed(process(53, None, None, None))
        };
        let (exit, fake) = run(vec![unread]);
        assert_eq!(exit, StopExit::NotStopped, "can_stop {can_stop}");
        assert_eq!(StopExit::NotStopped.as_code(), 48);
        let line = &fake.logged[0].1;
        assert!(
            line.contains("1 unknown (the process list couldn't be read)"),
            "{line}"
        );
    }
}

#[test]
fn the_log_line_names_a_failed_process_list_only_when_it_failed() {
    let unread = process(54, None, None, None);
    let (_, fake) = run(vec![unread.clone()]);
    let line = &fake.logged[0].1;
    assert!(line.contains("1 unknown, "), "{line}");
    assert!(!line.contains("process list"), "{line}");
    let failed = Process {
        list_failed: true,
        ..unread
    };
    let (_, fake) = run(vec![failed]);
    let line = &fake.logged[0].1;
    assert!(line.contains("the process list couldn't be read"), "{line}");
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
    // It isn't stopped, but the pool user's process beside it is; it may be any pool user's,
    // so every one is marked.
    assert_eq!(fake.stopped(), [10]);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    let (error, line) = &fake.logged[0];
    assert!(error);
    assert!(line.contains("1 unknown"), "{line}");
    assert!(line.contains("user couldn't be read"), "{line}");
}

#[test]
fn a_process_of_unknown_user_marks_every_pool_user_before_anything_is_stopped() {
    let mut fake = Fake::new(&[1010, 1011, 1012]);
    let first = vec![
        process(36, None, None, None),
        process(10, Some(sid(1010)), Some(sid(1010)), None),
    ];
    fake.snapshots = vec![Ok(first), Ok(vec![process(36, None, None, None)])];
    assert_eq!(stop(&mut fake), StopExit::NotStopped);
    assert_eq!(StopExit::NotStopped.as_code(), 48);
    // One mark of all three, in the first pass, before the pool user's process is suspended;
    // the second pass finds no one new to mark.
    assert_eq!(fake.done[1..4], ["snapshot", "mark 3", "suspend 10"]);
    assert_eq!(
        fake.done
            .iter()
            .filter(|line| line.starts_with("mark"))
            .count(),
        1
    );
    assert_eq!(fake.resets, [sid(1010), sid(1011), sid(1012)]);
    assert_eq!(fake.stopped(), [10]);
    let line = &fake.logged[0].1;
    assert!(
        line.contains("1 found running") && line.contains("1 unknown"),
        "{line}"
    );
}

#[test]
fn a_list_and_token_that_disagree_are_unknown() {
    let (exit, fake) = run(vec![process(35, Some(sid(1010)), Some(sid(1013)), None)]);
    assert_eq!(exit, StopExit::NotStopped);
    assert_eq!(fake.stopped(), Vec::<u32>::new());
    assert!(fake.logged[0].1.contains("1 unknown"), "{:?}", fake.logged);
}

/// A process no source names that can't be opened to be stopped, in `session`, with `image`.
fn unopened(session: u32, image: Image) -> Process {
    Process {
        can_stop: false,
        session: Some(session),
        image,
        ..process(36, None, None, None)
    }
}

/// The fake's Windows and Program Files folders, as `pick` takes them.
pub(super) fn folders() -> Vec<String> {
    [
        r"C:\WINDOWS",
        r"C:\Program Files",
        r"C:\Program Files (x86)",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn only_a_session_0_system_process_and_the_kernels_are_exempt_from_unknown() {
    let protected = unopened(0, Image::Path(r"C:\Windows\System32\csrss.exe".to_owned()));
    let unknown_kernel = [0, 4, OWN_PID].map(|pid| process(pid, None, None, None));
    let mut seen = vec![protected];
    seen.extend(unknown_kernel);
    let pool = [sid(1010)];
    assert_eq!(pick(&seen, &pool, OWN_PID, &folders()), Picked::default());
    let (exit, fake) = run(seen);
    assert_eq!(exit, StopExit::Done);
    assert!(fake.logged[0].1.contains("0 unknown"), "{:?}", fake.logged);
}

#[test]
fn an_unopenable_process_in_an_interactive_session_is_unknown_and_marks_every_pool_user() {
    // A pool user's process started protected: it runs in the lawyer's session, and neither
    // the list, the token nor the owner names it. Its image doesn't matter.
    let protected = unopened(1, Image::Path(r"C:\Windows\System32\csrss.exe".to_owned()));
    let (exit, fake) = run(vec![protected]);
    assert_eq!(exit, StopExit::NotStopped);
    assert_eq!(fake.resets, [sid(1010), sid(1011)]);
    assert!(fake.logged[0].1.contains("1 unknown"), "{:?}", fake.logged);
    assert_eq!(fake.completed(), None);
}

#[test]
fn a_session_0_unopenable_process_is_exempt_only_with_a_system_image() {
    let path = |path: &str| Image::Path(path.to_owned());
    let exempt = [
        path(r"C:\Windows\System32\lsass.exe"),
        path(r"c:\program files\Windows Defender\MsMpEng.exe"),
        path(r"\\?\C:\Program Files (x86)\a\b.exe"),
        Image::Missing,
    ];
    for image in exempt {
        let (exit, fake) = run(vec![unopened(0, image.clone())]);
        assert_eq!(exit, StopExit::Done, "{image:?}");
        assert!(fake.resets.is_empty(), "{image:?}");
    }
    // A signed binary copied where a user writes and started protected, by a pool user's
    // scheduled task (S4U) in session 0; a folder whose name only starts like a system one; a
    // path out of the folder; one that couldn't be read, or wasn't; or no session listed.
    let unknown = [
        path(r"C:\Users\x\AppData\Local\a.exe"),
        path(r"C:\Windows2\a.exe"),
        path(r"C:\Windows \a.exe"),
        path(r"C:\Windows\..\Users\x\a.exe"),
        path(r"\\server\share\Windows\a.exe"),
        path(r"C:\Windows"),
        Image::Failed,
        Image::Unread,
    ];
    for image in unknown {
        let (exit, fake) = run(vec![unopened(0, image.clone())]);
        assert_eq!(exit, StopExit::NotStopped, "{image:?}");
        assert_eq!(fake.resets, [sid(1010), sid(1011)], "{image:?}");
    }
    let unlisted = Process {
        session: None,
        ..unopened(0, Image::Missing)
    };
    let (exit, _) = run(vec![unlisted]);
    assert_eq!(exit, StopExit::NotStopped);
}

#[test]
fn system_image_matches_whole_components_of_the_folders_given() {
    let image = Image::Path(r"C:\Windows\a.exe".to_owned());
    assert!(system_image(&image, &[r"C:\Windows\".to_owned()]));
    assert!(system_image(&image, &[r"c:/windows".to_owned()]));
    assert!(!system_image(&image, &[r"C:\Win".to_owned()]));
    assert!(!system_image(&image, &[r"D:\Windows".to_owned()]));
    assert!(!system_image(&image, &[String::new()]));
    assert!(!system_image(&image, &[]));
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
    let picked = pick(&seen, &pool, OWN_PID, &folders());
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
