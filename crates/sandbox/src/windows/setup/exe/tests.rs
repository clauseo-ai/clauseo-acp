//! The setup exe on a real Windows. The unelevated tests run in CI's ordinary step. The
//! `elevated_` one runs only in CI's elevated step (`#[ignore]`, and skipped at run time unless
//! the token is elevated and `CLAUSEO_ELEVATED_TESTS=1`, D-B16): a full `--set-up --add 1`
//! against this family's own names (accounts `ClauseoSbTS<4 hex>NN`, the group
//! `ClauseoSbTSG<4 hex>`, the key `HKLM\SOFTWARE\Clauseo\SandboxTestSetup`), never the real
//! pool, then its removal. It first clears what a killed earlier try of this family left, by
//! prefix, and again when it ends, pass or fail.

use super::super::account_list::names_starting;
use super::super::accounts::{self, Kind};
use super::super::lock_tests::{suffix, unique_mutex};
use super::super::profile;
use super::super::strays::SETUP_TEST_PREFIX;
use super::*;
use crate::test_support::TempDir;
use crate::windows::marker::{self, MarkerState};
use crate::windows::record::current_user_sid;
use crate::windows::secrets;
use crate::windows::shared_writes::{
    plan_shared_write_denies, remove_shared_write_denies, shared_write_targets,
};
use crate::windows::test_acl::elevated_tests_enabled;

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
fn remove_exits_0_only_when_nothing_was_set_up() {
    let missing = format!("SOFTWARE\\Clauseo\\NoSuchKey{}", suffix());
    assert_eq!(remove(&missing), SetupExit::Done);
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
    };
    let dir = TempDir::new("setup-exe-elevated");
    let data = dir.path().join("data");
    let owner = Owner::of_this_process().unwrap();
    let me = owner.sid().clone();

    let exit = run_set_up(names.clone(), owner, &data, 1);
    let result = result_file::read(&data);
    assert_eq!(exit, SetupExit::Done, "{result:?}");
    assert_eq!(result.unwrap().unwrap().exit, 0);

    // The record: this user owns one pool user in the test group.
    let record = record_write::read(TEST_KEY).unwrap().unwrap();
    assert_eq!(record.owner, me);
    assert_eq!(record.pool.len(), 1);
    let user = format!("{users}01");
    let sid = accounts::sid_of(&user, Kind::User).unwrap().unwrap();
    assert_eq!(record.pool, std::slice::from_ref(&sid));
    assert_eq!(
        accounts::sid_of(&names.group, Kind::Group),
        Ok(Some(record.group.clone()))
    );
    assert_eq!(accounts::hidden(&user), Ok(true));
    // Its profile, with the `LocalLow` the deny closes.
    let profile = profile::profile_of(&sid).unwrap().unwrap();
    assert!(!profile.temporary);
    assert!(profile.path.join("AppData").join("LocalLow").is_dir());
    // The owner's files, written as the owner: the marker, and the password's blob.
    let MarkerState::Current(written) = marker::read(&data, &me) else {
        panic!("setup.json isn't current");
    };
    assert_eq!(written.pool, std::slice::from_ref(&sid));
    assert_eq!(written.group_sid, record.group);
    assert!(secrets::secret_for(&data, &sid).is_ok());

    // The removal, as PR4j will do it: the shared denies planned again from the record
    // (D-B11), then the profile, the account, the group and the record.
    let mut acl = NtfsAcl::new().unwrap();
    let program_data = super::super::ffi::program_data().unwrap();
    let targets = shared_write_targets(&program_data, std::slice::from_ref(&profile.path));
    let grants = plan_shared_write_denies(&record.group, &targets).unwrap();
    remove_shared_write_denies(&mut acl, &grants).unwrap();
    profile::delete_profile(&sid, &me).unwrap();
    assert_eq!(profile::profile_of(&sid), Ok(None));
    accounts::delete_user(&user).unwrap();
    assert_eq!(accounts::sid_of(&user, Kind::User), Ok(None));
    accounts::delete_group(&names.group).unwrap();
    record_write::delete(TEST_KEY).unwrap();
    assert_eq!(record_write::read(TEST_KEY), Ok(None));
}
