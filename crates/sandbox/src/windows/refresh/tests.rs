//! The refresh's replace sequence over the fake machine (`fake.rs`): the order, a crash at
//! every step, the self-repair of a record a crash left stale, and the failed steps.

use super::fake::*;
use super::*;

#[test]
fn a_signed_newer_exe_replaces_the_admin_copy_then_the_record() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.admin_file, Some(new()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&new())));
    assert_eq!(
        fake.set_aside,
        Some(old()),
        "kept until the reboot or the next run"
    );
    let order: Vec<_> = fake
        .calls
        .iter()
        .filter(|call| STEPS.contains(call))
        .collect();
    assert_eq!(order, STEPS.iter().collect::<Vec<_>>());
    assert_eq!(
        fake.logs,
        [(false, "the sandbox's admin copy was refreshed".to_owned())]
    );
}

#[test]
fn a_crash_at_any_step_never_leaves_a_hash_for_a_file_that_isnt_there() {
    let key = key();
    for crash in STEPS {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.crash = Some(crash);
        let _exit = run_fake(&mut fake, &key);
        assert!(fake.record_names_what_is_there(&old()), "crash at {crash}");
        assert_ne!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&new())),
            "crash at {crash}: the new hash is written last"
        );
        // The machine comes back and the task runs again (from whichever copy is in place):
        // it ends with the new copy, recorded.
        if fake.admin_file.is_none() {
            continue; // Only a re-adopt helps: the task's exe is gone (the launcher asks).
        }
        fake.crash = None;
        fake.crashed = false;
        assert_eq!(
            run_fake(&mut fake, &key),
            RefreshExit::Done,
            "after {crash}"
        );
        assert_eq!(fake.admin_file, Some(new()), "after {crash}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&new())),
            "after {crash}"
        );
    }
}

#[test]
fn a_new_copy_left_unrecorded_gets_its_hash_without_another_swap() {
    // A crash after the move left the new copy under the old hash.
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.admin_file = Some(new());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&new())));
    assert!(!fake.calls.contains(&"set_aside"));
    assert_eq!(fake.staged, None, "the staged copy is discarded");
}

#[test]
fn an_exe_already_recorded_changes_nothing() {
    let key = key();
    let mut fake = Fake::new(&new(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert!(!fake.calls.contains(&"set_aside") && !fake.calls.contains(&"write_sha256"));
    assert_eq!(fake.staged, None);
    assert_eq!(
        fake.logs,
        [(false, "the sandbox's admin copy is current".to_owned())]
    );
}

#[test]
fn a_failed_move_puts_the_old_copy_back() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("place_staged");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::ReplaceFailed);
    assert_eq!(fake.admin_file, Some(old()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&old())));
    assert_eq!(fake.staged, None, "the staged copy is discarded");
    let (error, line) = fake.logs.last().unwrap();
    assert!(
        *error && line.contains("the old copy is back in place"),
        "{line}"
    );
}

#[test]
fn a_failed_set_aside_or_securing_keeps_the_old_copy() {
    let key = key();
    for (fail, exit) in [
        ("set_aside", RefreshExit::ReplaceFailed),
        ("hold", RefreshExit::FolderBusy),
        ("secure_staged", RefreshExit::StagingFailed),
        ("stage", RefreshExit::StagingFailed),
    ] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.fail = Some(fail);
        assert_eq!(run_fake(&mut fake, &key), exit, "{fail}");
        assert_eq!(fake.admin_file, Some(old()), "{fail}");
        assert_eq!(
            fake.record.admin_copy_sha256,
            Some(sha_hex(&old())),
            "{fail}"
        );
        assert!(!fake.calls.contains(&"place_staged"), "{fail}");
    }
}

#[test]
fn a_failed_record_write_exits_its_own_code() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("write_sha256");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::RecordNotWritten);
    assert_eq!(fake.admin_file, Some(new()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&old())));
}

#[test]
fn the_folder_is_held_before_the_set_aside_and_a_failed_hold_moves_nothing() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    let at = |name| fake.calls.iter().position(|call| *call == name).unwrap();
    assert!(at("hold") < at("set_aside") && at("place_staged") < at("release"));
    assert!(at("release") < at("write_version"), "{:?}", fake.calls);
    assert!(!fake.holding());
    // Held through a failed move and its restore, then let go.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("place_staged");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::ReplaceFailed);
    assert!(fake.calls.contains(&"restore") && !fake.holding());
    // Someone holds the folder: nothing moves, and the staged copy is discarded.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("hold");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::FolderBusy);
    assert_eq!(fake.admin_file, Some(old()));
    assert_eq!((fake.set_aside.clone(), fake.staged.clone()), (None, None));
    assert!(!fake.calls.contains(&"set_aside") && !fake.calls.contains(&"place_staged"));
    assert_eq!(RefreshExit::FolderBusy.as_code(), 43);
}

#[test]
fn a_held_set_up_lock_reads_and_moves_nothing_and_a_run_holds_it_throughout() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.fail = Some("lock");
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::LockHeld);
    assert_eq!(RefreshExit::LockHeld.as_code(), 44);
    assert_eq!(fake.calls, ["is_system", "lock"]);
    assert_eq!(fake.admin_file, Some(old()));
    assert_eq!(fake.record.admin_copy_sha256, Some(sha_hex(&old())));
    // Taken before the record is read, given back after its last write, failed or not.
    for fail in [None, Some("place_staged"), Some("write_sha256")] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.fail = fail;
        let _exit = run_fake(&mut fake, &key);
        let at = |name| fake.calls.iter().position(|call| *call == name).unwrap();
        assert!(at("lock") < at("record"), "{fail:?}");
        assert_eq!(fake.calls.last(), Some(&"unlock"), "{fail:?}");
    }
}

#[test]
fn every_run_past_the_clear_leaves_no_staging_folder() {
    let key = key();
    // A swap: the set-aside copy is the running image, so it and the folder go at restart.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::Done);
    assert!(fake.calls.contains(&"leave_staging"));
    assert_eq!(
        fake.staging,
        Staging::AtRestart,
        "scheduled for deletion at restart"
    );
    // Nothing to swap, or a refusal or failure at any step after staging: the staged file is
    // discarded and the folder removed.
    let mut current = Fake::new(&old(), &old(), &key, &good_for(&old()));
    assert_eq!(run_fake(&mut current, &key), RefreshExit::Done);
    assert_eq!(current.staging, Staging::Absent, "{:?}", current.calls);
    for fail in ["stage", "secure_staged", "hold", "set_aside"] {
        let mut fake = Fake::new(&old(), &new(), &key, &good());
        fake.fail = Some(fail);
        assert_ne!(run_fake(&mut fake, &key), RefreshExit::Done, "{fail}");
        assert!(fake.calls.contains(&"leave_staging"), "{fail}");
        assert_eq!(fake.staging, Staging::Absent, "{fail}: {:?}", fake.calls);
    }
    // A source refused before staging: the run still ends by leaving no folder.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.source_exe = Err(SourceError::Missing);
    assert_eq!(run_fake(&mut fake, &key), RefreshExit::SourceRefused);
    assert!(fake.calls.contains(&"leave_staging"));
    assert_eq!(fake.staging, Staging::Absent);
}

#[test]
fn the_logon_policy_is_updated_under_the_lock_after_the_checks_only() {
    let key = key();
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    run_fake(&mut fake, &key);
    let at = |name: &str| fake.calls.iter().position(|call| *call == name).unwrap();
    let [update] = fake.policy_updates[..] else {
        panic!("{:?}", fake.policy_updates)
    };
    assert!(
        at("lock") < update && update <= at("unlock"),
        "{:?}",
        fake.calls
    );
    assert!(at("record") < update);
    // Not SYSTEM: refused before the lock, and nothing updated.
    let mut fake = Fake::new(&old(), &new(), &key, &good());
    fake.system = false;
    run_fake(&mut fake, &key);
    assert_eq!(fake.policy_updates, Vec::<usize>::new());
}
