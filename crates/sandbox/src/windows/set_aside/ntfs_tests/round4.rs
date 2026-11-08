//! Round 4 of the S15 `PR3b` review on NTFS: a name that isn't valid UTF-16 text, which NTFS
//! allows, is scanned, recorded, set aside and restored like any other.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;

use super::{after_session, before_launch, exit, launch, list_set_aside, restore_set_aside};
use crate::windows::set_aside::Reason;
use crate::windows::test_junction::junction;
use crate::windows::test_world::{World, matter_one};

/// Round 4, a name that isn't text: a junction named with a lone surrogate failed every
/// check, so it blocked every launch. Now the lawyer's one there at launch is recorded and
/// stays, and one the agent makes is set aside as itself under its own name, and restored.
#[test]
fn a_junction_named_with_a_lone_surrogate_is_recorded_and_set_aside_like_any_other() {
    let w = World::new("ntfs-aside-lone-surrogate");
    w.dir.dir("elsewhere/precedents");
    let target = w.canonical("elsewhere/precedents");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    let kept = root.join(OsString::from_wide(&[u16::from(b'K'), 0xD800]));
    junction(&kept, &target).unwrap();
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    let planted = root.join(OsString::from_wide(&[u16::from(b'P'), 0xDC00]));
    junction(&planted, &target).unwrap();
    let checked = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap();

    let seen: Vec<_> = checked
        .moved
        .iter()
        .map(|e| (e.from.clone(), e.reason))
        .collect();
    assert_eq!(seen, [(planted.clone(), Reason::New)]);
    assert_eq!(Some(checked.moved[0].name.as_os_str()), planted.file_name());
    assert!(
        std::fs::symlink_metadata(&kept).is_ok(),
        "the lawyer's link stays"
    );
    assert!(std::fs::symlink_metadata(&planted).is_err());
    assert_eq!(list_set_aside(&data, &matter_one()).unwrap(), checked.moved);
    restore_set_aside(&data, &checked.moved[0]).unwrap();
    assert!(std::fs::symlink_metadata(&planted).is_ok());
}
