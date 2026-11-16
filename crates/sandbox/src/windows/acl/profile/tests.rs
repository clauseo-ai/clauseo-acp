//! A profile's folders reached by handle, on real NTFS in a temp folder (no account needed):
//! a reparse point on the way to `LocalLow` is refused and what it names keeps its owner and
//! DACL; [`clear_links`] deletes each link and each extra name of a file itself, and nothing
//! they name. Directory and file symbolic links need the right to make them (an elevated token,
//! or Developer Mode): without it those canaries are skipped, named on stderr, and the junction
//! and hard-link canaries still run. The elevated tests with a real pool user's profile are in
//! `setup/logon/tests/links.rs`.

use std::os::windows::fs::MetadataExt as _;
use std::path::PathBuf;

use super::{Below, Cleared, clear_links, open_below, set_owner_below};
use crate::test_support::set_readonly;
use crate::windows::acl::{descriptor, owner};
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::Place;
use crate::windows::test_dacl::protect_with;
use crate::windows::test_junction::junction;

/// A canary's DACL: protected, so nothing it inherits changes under it.
const CANARY_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)";

/// `FILE_ATTRIBUTE_READONLY`.
const READONLY: u32 = 1;

/// A profile at `Users\Sb` in `place`, with `AppData\LocalLow` made as plain folders.
fn plain_profile(place: &Place) -> PathBuf {
    place.dir.dir("Users/Sb/AppData/LocalLow");
    place.at("Users/Sb")
}

/// A canary folder `canary` in `place` holding `LocalLow\keep.txt`, with [`CANARY_SDDL`] on it
/// and on `LocalLow`; its path, and `LocalLow`'s descriptor, owner included.
fn canary(place: &Place) -> (PathBuf, Vec<u8>) {
    place.dir.file("canary/LocalLow/keep.txt", "kept");
    let canary = place.at("canary");
    protect_with(&canary, CANARY_SDDL).unwrap();
    protect_with(&canary.join("LocalLow"), CANARY_SDDL).unwrap();
    let before = descriptor(&canary.join("LocalLow")).unwrap();
    (canary, before)
}

/// Whether a symbolic link could be made (`made`); a refusal for want of the right is printed,
/// and anything else fails the test.
fn made(what: &str, made: std::io::Result<PathBuf>) -> Option<PathBuf> {
    match made {
        Ok(path) => Some(path),
        // ERROR_PRIVILEGE_NOT_HELD: no elevated token and no Developer Mode.
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("skipped the {what} canary: {error}");
            None
        }
        Err(error) => panic!("{what}: {error}"),
    }
}

#[test]
fn a_plain_profiles_local_low_is_reached_and_its_owner_set() {
    let place = Place::new("profile-plain");
    let profile = plain_profile(&place);
    let local_low = profile.join("AppData").join("LocalLow");
    let me = current_user_sid().unwrap();
    assert!(open_below(&profile, &local_low).is_ok());
    set_owner_below(&profile, &local_low, &me).unwrap();
    assert_eq!(owner(&local_low).unwrap(), me);
}

#[test]
fn a_junction_at_app_data_is_refused_and_its_target_keeps_its_owner_and_dacl() {
    let place = Place::new("profile-junction-appdata");
    place.dir.dir("Users/Sb");
    let profile = place.at("Users/Sb");
    let (canary, before) = canary(&place);
    junction(&profile.join("AppData"), &canary).unwrap();
    let local_low = profile.join("AppData").join("LocalLow");
    let me = current_user_sid().unwrap();

    let refused = set_owner_below(&profile, &local_low, &me).unwrap_err();
    assert!(
        matches!(&refused, Below::Changed(part) if part == "AppData"),
        "{refused:?}"
    );
    assert!(matches!(
        open_below(&profile, &local_low),
        Err(Below::Changed(_))
    ));
    assert_eq!(descriptor(&canary.join("LocalLow")).unwrap(), before);
}

#[test]
fn a_directory_symlink_at_app_data_is_refused_and_its_target_keeps_its_owner_and_dacl() {
    let place = Place::new("profile-symlink-appdata");
    place.dir.dir("Users/Sb");
    let profile = place.at("Users/Sb");
    let (canary, before) = canary(&place);
    let Some(_) = made(
        "directory symlink",
        place.dir.symlink_dir("Users/Sb/AppData", &canary),
    ) else {
        return;
    };
    let local_low = profile.join("AppData").join("LocalLow");
    let refused = set_owner_below(&profile, &local_low, &current_user_sid().unwrap());
    assert!(
        matches!(&refused, Err(Below::Changed(part)) if part == "AppData"),
        "{refused:?}"
    );
    assert_eq!(descriptor(&canary.join("LocalLow")).unwrap(), before);
}

#[test]
fn a_junction_at_local_low_itself_is_refused() {
    let place = Place::new("profile-junction-locallow");
    place.dir.dir("Users/Sb/AppData");
    let profile = place.at("Users/Sb");
    let (canary, before) = canary(&place);
    let local_low = profile.join("AppData").join("LocalLow");
    junction(&local_low, &canary.join("LocalLow")).unwrap();
    let refused = set_owner_below(&profile, &local_low, &current_user_sid().unwrap());
    assert!(
        matches!(&refused, Err(Below::Changed(part)) if part == "LocalLow"),
        "{refused:?}"
    );
    assert_eq!(descriptor(&canary.join("LocalLow")).unwrap(), before);
}

#[test]
fn a_profile_folder_that_is_a_junction_is_refused() {
    let place = Place::new("profile-junction-root");
    let real = plain_profile(&place);
    place.dir.dir("Links");
    let profile = place.at("Links/Sb");
    junction(&profile, &real).unwrap();
    let local_low = profile.join("AppData").join("LocalLow");
    let refused = open_below(&profile, &local_low).unwrap_err();
    assert!(
        matches!(&refused, Below::Changed(part) if part == "the profile folder"),
        "{refused:?}"
    );
}

#[test]
fn a_missing_local_low_is_missing_and_a_folder_outside_the_profile_is_refused() {
    let place = Place::new("profile-missing");
    place.dir.dir("Users/Sb/AppData");
    let profile = place.at("Users/Sb");
    let local_low = profile.join("AppData").join("LocalLow");
    assert!(matches!(
        open_below(&profile, &local_low),
        Err(Below::Missing)
    ));
    assert!(matches!(
        open_below(&profile, &place.at("elsewhere")),
        Err(Below::Io(_))
    ));
}

#[test]
fn clearing_a_profile_deletes_each_link_itself_and_nothing_it_names() {
    let place = Place::new("profile-clear");
    let profile = plain_profile(&place);
    let (canary, before) = canary(&place);
    let canary_file = canary.join("LocalLow").join("keep.txt");
    let file_before = descriptor(&canary_file).unwrap();
    set_readonly(&canary_file, true);
    // The profile's own content, which stays for the profile service.
    place.dir.file("Users/Sb/AppData/Local/Deep/own.txt", "own");
    let deep = profile.join("AppData").join("Local").join("Deep");
    let mut links = vec![
        profile.join("Junction"),
        deep.join("Junction"),
        place
            .dir
            .hard_link("Users/Sb/AppData/LocalLow/Linked.txt", &canary_file),
    ];
    junction(&links[0], &canary).unwrap();
    junction(&links[1], &canary.join("LocalLow")).unwrap();
    let mut expected = Cleared {
        links: 2,
        linked_files: 1,
    };
    let symlinks = [
        made(
            "directory symlink",
            place
                .dir
                .symlink_dir("Users/Sb/AppData/Local/Deep/DirLink", &canary),
        ),
        made(
            "file symlink",
            place
                .dir
                .symlink_file("Users/Sb/FileLink.txt", &canary_file),
        ),
    ];
    for link in symlinks.into_iter().flatten() {
        links.push(link);
        expected.links += 1;
    }

    assert_eq!(clear_links(&profile).unwrap(), expected);
    for link in &links {
        assert!(
            std::fs::symlink_metadata(link).is_err(),
            "{}",
            link.display()
        );
    }
    assert_eq!(
        std::fs::read_to_string(deep.join("own.txt")).unwrap(),
        "own"
    );
    assert_eq!(std::fs::read_to_string(&canary_file).unwrap(), "kept");
    assert_eq!(descriptor(&canary.join("LocalLow")).unwrap(), before);
    assert_eq!(descriptor(&canary_file).unwrap(), file_before);
    let attributes = std::fs::metadata(&canary_file).unwrap().file_attributes();
    assert_ne!(attributes & READONLY, 0, "the canary is still read-only");
    // Cleared again: nothing left to delete.
    assert_eq!(clear_links(&profile).unwrap(), Cleared::default());
    set_readonly(&canary_file, false);
}
