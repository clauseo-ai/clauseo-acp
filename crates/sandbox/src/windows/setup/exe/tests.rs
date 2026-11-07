//! The setup exe on a real Windows. The unelevated tests run in CI's ordinary step. The
//! `elevated_` one runs only in CI's elevated step (`#[ignore]`, and skipped at run time unless
//! the token is elevated and `CLAUSEO_ELEVATED_TESTS=1`, D-B16): a full `--set-up --add 1`
//! (topped up to [`MIN_FREE_AFTER_RUN`](super::super::MIN_FREE_AFTER_RUN) users) against this
//! family's own names (accounts `ClauseoSbTS<4 hex>NN`, the group `ClauseoSbTSG<4 hex>`, the
//! key `HKLM\SOFTWARE\Clauseo\SandboxTestSetup`), never the real pool, then its removal by
//! `--remove`'s own code, as the owner (`PR4j`). It first clears what a killed earlier try of this
//! family left, by prefix, and again when it ends, pass or fail.

use super::super::account_list::names_starting;
use super::super::accounts::{self, Kind};
use super::super::lock_tests::{suffix, unique_mutex};
use super::super::profile;
use super::super::record_write;
use super::super::strays::SETUP_TEST_PREFIX;
use super::*;
use crate::test_support::TempDir;
use crate::windows::marker::{self, MarkerState};
use crate::windows::record::current_user_sid;
use crate::windows::secrets;
use crate::windows::test_acl::{elevated_tests_enabled, has};
use crate::windows::wfp::{self, Namespace};

/// This family's record key.
const TEST_KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestSetup";

#[test]
fn a_wrong_command_line_exits_usage_and_touches_nothing() {
    for line in [
        &[][..],
        &["--set-up"][..],
        &["--remove", "--parent", "1"][..],
    ] {
        let exit = main(line.iter().map(OsString::from));
        assert_eq!(exit, SetupExit::Usage, "{line:?}");
    }
}

#[test]
fn remove_exits_0_when_nothing_was_set_up_and_no_set_up_holds_the_lock() {
    // With no record, both modes only try the lock, without waiting, and end, with or without
    // someone who asked. The lock's namespace needs an elevated token: skipped without one.
    let Some(mutex) = unique_mutex() else {
        return;
    };
    let run = suffix();
    let names = Names {
        record_key: format!("SOFTWARE\\Clauseo\\NoSuchKey{run}"),
        group: format!("{SETUP_TEST_PREFIX}G{run}"),
        mutex,
        users: SETUP_TEST_PREFIX.to_owned(),
        journals: false,
        wfp: Namespace::numbered_test(1),
    };
    for per_machine in [false, true] {
        assert_eq!(
            run_remove(names.clone(), None, per_machine),
            SetupExit::Removed
        );
        let asker = Owner::of_this_process().unwrap();
        assert_eq!(
            run_remove(names.clone(), Some(asker), per_machine),
            SetupExit::Removed
        );
    }
}

#[test]
fn a_parent_that_isnt_the_recorded_one_writes_no_result() {
    let dir = TempDir::new("setup-exe-parent");
    let data = dir.path().join("data");
    let line = [
        "--set-up".to_owned(),
        "--parent".to_owned(),
        std::process::id().to_string(),
        "--data-dir".to_owned(),
        data.display().to_string(),
    ];
    assert_eq!(main(line.map(OsString::from)), SetupExit::Failed);
    assert_eq!(result_file::read(&data), Ok(None));
}

/// Deletes every account whose name has this family's prefix, with its profile, every group
/// with it, and the test key. Each step goes on past a failure.
fn sweep() {
    let me = current_user_sid().unwrap();
    for name in names_starting(SETUP_TEST_PREFIX, Kind::User).unwrap_or_default() {
        if let Ok(Some(sid)) = accounts::sid_of(&name, Kind::User) {
            drop(profile::delete_profile(&sid, &me));
        }
        drop(accounts::delete_user(&name));
    }
    for name in names_starting(SETUP_TEST_PREFIX, Kind::Group).unwrap_or_default() {
        drop(accounts::delete_group(&name));
    }
    drop(record_write::delete(TEST_KEY));
    // The set-up's network step (S15 PR7) wrote in its own test namespace.
    drop(wfp::remove(&Namespace::numbered_test(1), Some(&me)));
}

/// Sweeps when dropped, so a failed assertion still cleans up.
struct Sweep;

impl Drop for Sweep {
    fn drop(&mut self) {
        sweep();
    }
}

#[test]
#[ignore = "elevated: CI's elevated step runs it (CLAUSEO_ELEVATED_TESTS=1)"]
fn elevated_set_up_adds_one_user_then_removal_takes_it_all_off() {
    if !elevated_tests_enabled() {
        return;
    }
    let Some(mutex) = unique_mutex() else {
        return;
    };
    sweep();
    let _sweep = Sweep;
    let run: String = suffix().chars().take(4).collect();
    let users = format!("{SETUP_TEST_PREFIX}{run}");
    let names = Names {
        record_key: TEST_KEY.to_owned(),
        group: format!("{SETUP_TEST_PREFIX}G{run}"),
        mutex,
        users: users.clone(),
        journals: false,
        wfp: Namespace::numbered_test(1),
    };
    let dir = TempDir::new("setup-exe-elevated");
    let owner = Owner::of_this_process().unwrap();
    let me = owner.sid().clone();
    // The app makes its data folder as the lawyer, who owns it. An elevated test process makes
    // folders owned by Administrators (its token's default owner), so hand this one to the user,
    // as the set-up's data-folder check requires.
    let data = dir.dir("data");
    crate::windows::test_dacl::give_owner(&data, &me).unwrap();

    let exit = run_set_up(names.clone(), owner, &data, 1);
    let result = result_file::read(&data);
    assert_eq!(exit, SetupExit::Done, "{result:?}");
    assert_eq!(result.unwrap().unwrap().exit, 0);

    // The record: this user owns the pool in the test group, the one asked for topped up to the
    // free users every run leaves.
    let record = record_write::read(TEST_KEY).unwrap().unwrap();
    assert_eq!(record.owner, me);
    assert_eq!(record.pool.len(), super::super::MIN_FREE_AFTER_RUN);
    let user = format!("{users}01");
    let sid = accounts::sid_of(&user, Kind::User).unwrap().unwrap();
    assert_eq!(record.pool[0], sid);
    assert_eq!(
        accounts::sid_of(&names.group, Kind::Group),
        Ok(Some(record.group.clone()))
    );
    assert_eq!(accounts::hidden(&user), Ok(true));
    // Its profile, with the `LocalLow` the deny closes.
    let profile = profile::profile_of(&sid).unwrap().unwrap();
    assert!(!profile.temporary);
    assert!(profile.path.join("AppData").join("LocalLow").is_dir());
    // No pool user owns its `LocalLow` (item 11): it would keep an owner's `WRITE_DAC` there.
    let local_low_owner =
        crate::windows::acl::owner(&profile.path.join("AppData").join("LocalLow")).unwrap();
    assert_ne!(
        local_low_owner, sid,
        "the pool user doesn't own its LocalLow"
    );
    // Nor DeviceSync, whose deny the group gets (the owner audit).
    let device_sync = super::super::ffi::program_data()
        .unwrap()
        .join("Microsoft")
        .join("DeviceSync");
    if device_sync.is_dir() {
        let owner = crate::windows::acl::owner(&device_sync).unwrap();
        assert!(owner != sid && owner != record.group, "{owner}");
    }
    // The owner's files, written as the owner: the marker, and the password's blob.
    let MarkerState::Current(written) = marker::read(&data, &me) else {
        panic!("setup.json isn't current");
    };
    assert_eq!(written.pool, record.pool);
    assert_eq!(written.group_sid, record.group);
    for each in &record.pool {
        assert!(secrets::secret_for(&data, each).is_ok(), "{each}");
    }

    // The removal, as the uninstaller's per-user `--remove` runs it for the owner: the shared
    // denies planned again from the record (D-B11), each profile, each account with its sign-in
    // screen value, the group and the record.
    let device_sync = super::super::ffi::program_data()
        .unwrap()
        .join("Microsoft")
        .join("DeviceSync");
    let asker = Owner::of_this_process().unwrap();
    assert_eq!(
        run_remove(names.clone(), Some(asker), false),
        SetupExit::Removed
    );
    for (n, each) in (1..).zip(&record.pool) {
        let user = format!("{users}{n:02}");
        assert_eq!(profile::profile_of(each), Ok(None), "{user}");
        assert_eq!(accounts::sid_of(&user, Kind::User), Ok(None), "{user}");
        assert_eq!(accounts::hidden(&user), Ok(false), "{user}");
    }
    assert_eq!(accounts::sid_of(&names.group, Kind::Group), Ok(None));
    assert_eq!(record_write::read(TEST_KEY), Ok(None));
    if device_sync.is_dir() {
        assert!(!has(&device_sync, record.group.as_str(), false));
    }
    // Again: nothing is left, so it ends at once.
    let asker = Owner::of_this_process().unwrap();
    assert_eq!(run_remove(names, Some(asker), false), SetupExit::Removed);
}
