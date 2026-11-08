//! The drive folder on real NTFS, in a temp folder standing in for a volume's root: one another
//! account owns is refused and kept as it is ([`make_top`]); a link at its name is refused by
//! the heal and left; a squatted one is set aside by its handle as `SYSTEM` would, with what it
//! holds unchanged, and the lawyer's made afresh. The heal needs the backup, restore and
//! take-ownership privileges, so its tests are `elevated_` ones.

use std::time::SystemTime;

use super::super::super::{owner, set_owner};
use super::super::{drive_sddl, make_top};
use super::{Ntfs, enable_privileges};
use crate::windows::acl::descriptor;
use crate::windows::record::current_user_sid;
use crate::windows::set_aside::drive_folder_name;
use crate::windows::set_aside::heal::{Healed, heal};
use crate::windows::sids::Sid;
use crate::windows::test_acl::{Place, elevated_tests_enabled};
use crate::windows::test_junction::junction;

/// `Administrators`, the owner an elevated test gives a squatted folder.
const ADMINISTRATORS: &str = "S-1-5-32-544";

#[test]
fn elevated_a_drive_folder_another_account_owns_is_refused_and_not_taken_back() {
    if !elevated_tests_enabled() {
        return;
    }
    let place = Place::new("drive-not-yours");
    let me = current_user_sid().unwrap();
    let folder = place.dir.dir(&drive_folder_name(me.as_str()));
    let admins = Sid::parse(ADMINISTRATORS).unwrap();
    set_owner(&folder, &admins).unwrap();

    let refused = make_top(&folder, &me, &drive_sddl(me.as_str(), None)).unwrap_err();

    assert!(
        refused
            .get_ref()
            .is_some_and(|inner| inner.to_string().contains("another account")),
        "{refused}"
    );
    assert_eq!(owner(&folder).unwrap(), admins, "not taken back");
}

#[test]
fn a_drive_folder_made_by_the_lawyer_is_the_lawyers() {
    let place = Place::new("drive-made");
    let me = current_user_sid().unwrap();
    let folder = place.at(&drive_folder_name(me.as_str()));
    make_top(&folder, &me, &drive_sddl(me.as_str(), None)).unwrap();
    make_top(&folder, &me, &drive_sddl(me.as_str(), None)).unwrap();
    assert_eq!(owner(&folder).unwrap(), me);
}

#[test]
fn a_link_at_the_drive_folders_name_is_refused_by_the_heal_and_left() {
    let place = Place::new("drive-heal-link");
    let target = place.dir.file("elsewhere/keep.txt", "kept");
    let target_before = descriptor(&target).unwrap();
    let root = place.at("");
    let lawyer = Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let link = root.join(drive_folder_name(lawyer.as_str()));
    junction(&link, &place.at("elsewhere")).unwrap();

    let healed = heal(&Ntfs, &root, &lawyer, SystemTime::UNIX_EPOCH);

    assert_eq!(healed, Healed::Refused);
    assert!(std::fs::symlink_metadata(&link).is_ok(), "the link stays");
    assert_eq!(descriptor(&target).unwrap(), target_before);
}

#[test]
fn elevated_a_squatted_drive_folder_is_set_aside_by_its_handle_and_made_afresh() {
    if !elevated_tests_enabled() {
        return;
    }
    enable_privileges().unwrap();
    let place = Place::new("drive-heal");
    let me = current_user_sid().unwrap();
    let name = drive_folder_name(me.as_str());
    let child = place
        .dir
        .file(&format!("{name}/sub/brief.txt"), "a squatter's");
    let child_before = descriptor(&child).unwrap();
    let root = place.at("");
    let folder = root.join(&name);
    set_owner(&folder, &Sid::parse(ADMINISTRATORS).unwrap()).unwrap();

    let healed = heal(&Ntfs, &root, &me, SystemTime::UNIX_EPOCH);

    let aside = format!("{name}.squatted-19700101T000000Z");
    assert_eq!(healed, Healed::SetAside(aside.clone()));
    let moved = root.join(&aside).join("sub").join("brief.txt");
    assert_eq!(
        descriptor(&moved).unwrap(),
        child_before,
        "inside is untouched"
    );
    assert_eq!(owner(&root.join(&aside)).unwrap().as_str(), "S-1-5-18");
    assert_eq!(owner(&folder).unwrap(), me, "the fresh one is the lawyer's");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
}
