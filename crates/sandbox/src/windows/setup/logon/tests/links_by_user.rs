//! What a pool user does in its own profile, done by a process running as the user
//! ([`run_in_cmd`]): the profile delete takes out the junction and the extra name of a file the
//! user planted, so what they name keeps its content, owner, DACL and attributes; and a profile
//! whose `AppData` the user shut to `Administrators` is still reached, handed off and deleted,
//! the set-up's backup and restore privileges letting it through.
//!
//! `elevated_` and `#[ignore]`, behind [`gate`], with the accounts of `links.rs`
//! ([`user_with_profile`]).

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::os::windows::fs::MetadataExt as _;
use std::path::{Component, Path};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::super::super::accounts_tests::machine_on;
use super::super::super::machine::{Machine, MachineError};
use super::super::super::profile::{delete_profile, profile_of};
use super::super::super::windows_machine::Names;
use super::as_user::run_in_cmd;
use super::links::{ADMINISTRATORS, Canary, user_with_profile, wait_for_hive};
use super::{gate, me};
use crate::test_support::{TempDir, set_readonly, symlink_dir_at, symlink_file_at};
use crate::windows::acl::object::Object;

/// The drive or share `path` starts with, without case: a hard link joins two names on one.
fn volume(path: &Path) -> Option<String> {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            Some(prefix.as_os_str().to_string_lossy().to_lowercase())
        }
        _ => None,
    }
}

/// Whether `path` is a reparse point itself (not followed).
fn is_reparse_point(path: &Path) -> bool {
    let attributes = std::fs::symlink_metadata(path).unwrap().file_attributes();
    attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

/// Whether `path`'s read-only attribute is set.
fn read_only(path: &Path) -> bool {
    std::fs::metadata(path).unwrap().permissions().readonly()
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_profile_delete_leaves_what_the_users_links_name_as_it_was() {
    if !gate("elevated_a_profile_delete_leaves_what_the_users_links_name_as_it_was") {
        return;
    }
    let dir = TempDir::new("links-profile-delete");
    let (user, profile) = user_with_profile();
    let canary = Canary::new(&dir, Some(&user.sid));
    let junction = profile.join("Junction");
    let local = profile.join("AppData").join("Local");
    let linked = local.join("Linked.txt");
    let mut script = format!(
        r#"mklink /J "{}" "{}""#,
        junction.display(),
        canary.folder.display()
    );
    let same_volume = volume(&profile) == volume(&canary.file);
    if same_volume {
        write!(
            script,
            r#" && mklink /H "{}" "{}""#,
            linked.display(),
            canary.file.display()
        )
        .unwrap();
    } else {
        eprintln!("skipped the hard link: the canary is on another volume than the profile");
    }
    // The user plants these in its own profile, and its process has ended when this returns.
    run_in_cmd(&user, &profile, &script);
    assert!(is_reparse_point(&junction), "{}", junction.display());
    if same_volume {
        assert_eq!(Object::open(&canary.file).unwrap().links, 2);
    }
    // Symbolic links need SeCreateSymbolicLinkPrivilege, which a pool user lacks: this
    // (elevated) process plants them in the user's profile instead.
    symlink_dir_at(&local.join("DirLink"), &canary.folder);
    symlink_file_at(&profile.join("FileLink.txt"), &canary.file);
    // The attribute belongs to the file, so every name shares it: a sweep that clears it to take
    // a name out must set it back.
    set_readonly(&canary.file, true);

    wait_for_hive(&user.sid, false, None);
    delete_profile(&user.sid, &me()).unwrap();
    assert!(!profile.exists(), "{}", profile.display());
    assert_eq!(profile_of(&user.sid).unwrap(), None);
    canary.assert_unchanged();
    assert!(read_only(&canary.file), "{}", canary.file.display());
    // Cleared so the scratch folder's removal on drop isn't refused.
    set_readonly(&canary.file, false);
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_profile_shut_to_administrators_is_still_handed_off_and_deleted() {
    if !gate("elevated_a_profile_shut_to_administrators_is_still_handed_off_and_deleted") {
        return;
    }
    let (user, profile) = user_with_profile();
    let app_data = profile.join("AppData");
    // The user may change its own folders' DACLs: it denies `Administrators` everything on
    // `AppData` and, through inheritance, what is below it.
    run_in_cmd(
        &user,
        &profile,
        &format!(
            r#"icacls "{}" /deny *{ADMINISTRATORS}:(OI)(CI)F"#,
            app_data.display()
        ),
    );
    wait_for_hive(&user.sid, false, None);
    // This process holds the backup and restore privileges disabled: the deny shuts it out.
    let shut = std::fs::read_dir(&app_data).unwrap_err();
    assert_eq!(shut.kind(), ErrorKind::PermissionDenied, "{shut}");

    let local_low = app_data.join("LocalLow");
    let mut machine = machine_on(Names::production(), Path::new(r"C:\ClauseoNoSuchData"));
    let owner = machine.owner_in_profile(&profile, &local_low).unwrap();
    let owner = owner.unwrap_or_else(|| panic!("{} wasn't found", local_low.display()));
    eprintln!("LocalLow's owner: {}", owner.as_str());
    match machine.give_to_admins(&profile, &local_low) {
        Ok(()) => {}
        Err(MachineError::ProfileChanged(part)) => eprintln!("the hand-off stopped at {part}"),
        Err(other) => panic!("the hand-off failed: {other:?}"),
    }

    delete_profile(&user.sid, &me()).unwrap();
    assert!(!profile.exists(), "{}", profile.display());
    assert_eq!(profile_of(&user.sid).unwrap(), None);
}
