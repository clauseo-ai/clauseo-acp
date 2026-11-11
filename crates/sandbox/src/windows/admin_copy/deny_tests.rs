//! The admin copy's deny of the pool's group (`locked.rs`): the SDDL with and without a
//! recorded group, the exact read-back `--adopt` and the set-up require, and the launch check,
//! which needs the deny on `Sandbox` and the copy whenever the record names a group.

use std::path::PathBuf;

use super::fake::{Fake, deny_last, locked_dacl, locked_dacl_denying};
use super::*;

const GROUP: &str = "S-1-5-21-1-2-3-1100";
const OTHER_GROUP: &str = "S-1-5-21-1-2-3-1200";

fn group() -> Sid {
    Sid::parse(GROUP).unwrap()
}

fn sandbox() -> PathBuf {
    admin_dir(Path::new(r"C:\Program Files"))
}

fn exe() -> PathBuf {
    admin_exe(Path::new(r"C:\Program Files"))
}

#[test]
fn the_sddl_denies_the_group_first_only_when_there_is_one() {
    assert_eq!(
        locked_file_sddl(None),
        "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)"
    );
    assert_eq!(
        locked_folder_sddl(None),
        "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)"
    );
    assert_eq!(
        locked_file_sddl(Some(&group())),
        format!("O:BAD:P(D;;FA;;;{GROUP})(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)")
    );
    assert_eq!(
        locked_folder_sddl(Some(&group())),
        format!(
            "O:BAD:P(D;OICI;FA;;;{GROUP})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)"
        )
    );
}

#[test]
fn locked_requires_the_deny_with_a_group_and_takes_none_without() {
    let other = Sid::parse(OTHER_GROUP).unwrap();
    for folder in [true, false] {
        let plain = locked_dacl(folder);
        let denying = locked_dacl_denying(folder, GROUP);
        assert!(locked(&plain, folder, None), "no group: no deny");
        assert!(
            !locked(&plain, folder, Some(&group())),
            "a group: the deny is missing"
        );
        assert!(locked(&denying, folder, Some(&group())), "a group: denied");
        assert!(
            !locked(&denying, folder, None),
            "no group: exactly the three"
        );
        assert!(
            !locked(&denying, folder, Some(&other)),
            "another group's deny"
        );
        assert!(
            !locked(&denying, !folder, Some(&group())),
            "the other inheritance"
        );
        assert!(
            !locked(&deny_last(&denying), folder, Some(&group())),
            "deny last"
        );
    }
}

#[test]
fn the_check_requires_the_deny_on_the_folder_and_the_copy_once_a_group_is_recorded() {
    // No recorded group: the no-deny form passes, as does a deny left from a removed pool.
    let mut fake = Fake::ready();
    assert_eq!(check(&fake), Ok(exe()));
    fake.set(
        &sandbox(),
        Found::Folder,
        locked_dacl_denying(true, OTHER_GROUP),
    );
    assert_eq!(check(&fake), Ok(exe()));
    // A recorded group with no deny reads as not locked: the installer's repair locks again.
    let mut fake = Fake::ready();
    fake.group = Some(group());
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
    fake.set(&sandbox(), Found::Folder, locked_dacl_denying(true, GROUP));
    assert_eq!(
        check(&fake),
        Err(Blocked::AdminCopyUnsafe),
        "the copy has none"
    );
    fake.set(&exe(), Found::File, locked_dacl_denying(false, GROUP));
    assert_eq!(check(&fake), Ok(exe()));
    // Another group's deny, or the group's deny after the allows, doesn't count.
    fake.set(&exe(), Found::File, locked_dacl_denying(false, OTHER_GROUP));
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "another group");
    let last = deny_last(&locked_dacl_denying(false, GROUP));
    assert!(level_safe(&last) && !group_denied(&last, &group()));
    fake.set(&exe(), Found::File, last);
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "deny last");
    // A group that can't be read fails closed, as the record's other values do.
    let mut fake = Fake::ready();
    fake.group_unreadable = true;
    assert_eq!(check(&fake), Err(Blocked::AdminCopyMissing));
}

#[test]
fn a_partial_or_inherit_only_deny_isnt_the_groups_deny() {
    let partial = fake::descriptor(
        ADMINISTRATORS,
        Some(&[(1, 0, WRITE_DATA, GROUP), (0, 0, READ_EXECUTE, USERS)]),
        true,
    );
    assert!(!group_denied(&partial, &group()));
    let inherit_only = fake::descriptor(
        ADMINISTRATORS,
        Some(&[(1, 0xB, FULL_ACCESS, GROUP), (0, 0x3, READ_EXECUTE, USERS)]),
        true,
    );
    assert!(!group_denied(&inherit_only, &group()));
    let generic = fake::descriptor(
        ADMINISTRATORS,
        Some(&[(1, 0, GENERIC_ALL, GROUP), (0, 0, READ_EXECUTE, USERS)]),
        true,
    );
    assert!(group_denied(&generic, &group()));
}
