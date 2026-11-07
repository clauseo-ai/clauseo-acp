//! S15 `PR4i` on a real NTFS volume: the hand-back after `--reclaim` (`strip_taken`) takes off a
//! lawyer-owned item only the explicit allows of pool users other than the matter's own: a broad
//! group's allow, the matter's own pool user's, the lawyer's and the DACL's protection stay, and
//! so does an allow the ledger lists as ours (`keep`).

use super::*;
use crate::windows::test_acl::{entries, owner_view};

const OTHER: &str = "S-1-5-21-1111111111-2222222222-3333333333-1002";

fn other() -> Sid {
    Sid::parse(OTHER).unwrap()
}

#[test]
fn only_another_pool_users_allow_goes_and_everything_else_stays() {
    drop_privileges();
    let place = Place::new("acl-strip");
    let me = current_user_sid().unwrap();
    let item = dunce::canonicalize(place.dir.file("matter/sub/taken.txt", "x")).unwrap();
    let dacl = format!("D:P(A;;FA;;;{me})(A;;FR;;;WD)(A;;FA;;;{USER_SID})(A;;FA;;;{OTHER})");
    protect_with(&item, &dacl).unwrap();

    let mut acl = NtfsAcl::new().unwrap();
    let walked = acl
        .strip_taken(&place.at("matter"), &[other()], &[], true)
        .unwrap();

    assert_eq!(walked.repaired, 1);
    assert_eq!(owner(&item).unwrap(), me);
    assert!(protected(&item), "its protection stays");
    assert!(!has(&item, OTHER, false), "another pool user's allow goes");
    for kept in [me.as_str(), "S-1-1-0", USER_SID] {
        assert!(has(&item, kept, false), "{kept} stays");
    }
}

/// A taken root (`PoolOwnedRoot`), alone: the matter's own `Modify` stays, another pool user's
/// allow goes, and nothing below it is touched.
#[test]
fn a_taken_root_keeps_the_matters_own_modify_and_loses_another_users_allow() {
    drop_privileges();
    let place = Place::new("acl-strip-root");
    let root = dunce::canonicalize(place.dir.dir("matter")).unwrap();
    let child = dunce::canonicalize(place.dir.file("matter/child.txt", "x")).unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    acl.add(&grant(&root, USER_SID, Rights::Modify, Inherit::Tree))
        .unwrap();
    acl.add(&grant(&root, OTHER, Rights::Modify, Inherit::ThisOnly))
        .unwrap();
    auto_inherit_with(&child, &format!("D:(A;;FA;;;{OTHER})")).unwrap();

    acl.strip_taken(&root, &[other()], &[], false).unwrap();

    assert!(has(&root, USER_SID, false), "the matter's own Modify stays");
    assert!(!has(&root, OTHER, false));
    assert!(
        has(&child, OTHER, false),
        "a root alone: its children are untouched"
    );
}

/// Our own allows the ledger lists (a credential link's, another matter's root grant) stay,
/// whoever they name.
#[test]
fn an_allow_the_ledger_lists_as_ours_stays() {
    drop_privileges();
    let place = Place::new("acl-strip-keep");
    let link = dunce::canonicalize(place.dir.file("state/.credentials.json", "{}")).unwrap();
    let stray = dunce::canonicalize(place.dir.file("state/stray.txt", "x")).unwrap();
    for item in [&link, &stray] {
        auto_inherit_with(item, &format!("D:(A;;FA;;;{OTHER})")).unwrap();
    }
    let keep = [(link.clone(), other())];

    let mut acl = NtfsAcl::new().unwrap();
    acl.strip_taken(&place.at("state"), &[other()], &keep, true)
        .unwrap();

    assert!(has(&link, OTHER, false), "ours stays");
    assert!(!has(&stray, OTHER, false));
}

/// What another user owns is never touched, whatever it allows.
#[test]
fn an_item_the_lawyer_doesnt_own_is_left_alone() {
    drop_privileges();
    let place = Place::new("acl-strip-others");
    let mine = dunce::canonicalize(place.dir.file("matter/mine.txt", "x")).unwrap();
    auto_inherit_with(&mine, &format!("D:(A;;FA;;;{OTHER})")).unwrap();
    let before = entries(&mine);

    let walked = NtfsAcl::new()
        .unwrap()
        .strip_taken(
            &place.at("matter"),
            &[Sid::parse(USER_SID).unwrap()],
            &[],
            true,
        )
        .unwrap();

    assert_eq!(walked.repaired, 0, "no allow for the SIDs it strips");
    assert_eq!(entries(&mine), before);
}

/// An item the agent shut with only its own entry (what `--reclaim` took) is unprotected once
/// stripped: it takes the matter's inherited entries, the fence's deny included, so the next
/// strict walk passes it. One where a colleague's explicit entry is left keeps its protection
/// (a taken root of its own, outside the matter, so the strict walk doesn't meet it); with only
/// the colleague let in, it is read as its owner alone (`owner_view`).
#[test]
fn a_shut_item_of_pool_entries_alone_inherits_and_a_colleagues_keeps_its_protection() {
    drop_privileges();
    let place = Place::new("acl-strip-bare");
    let matter = place.dir.dir("matter");
    let shut = dunce::canonicalize(place.dir.file("matter/shut.txt", "x")).unwrap();
    let shared = dunce::canonicalize(place.dir.file("elsewhere/shared.txt", "x")).unwrap();
    let colleague = "S-1-5-21-9-9-9-1500";
    protect_with(&shut, &format!("D:P(A;;FA;;;{OTHER})")).unwrap();
    protect_with(
        &shared,
        &format!("D:P(A;;FA;;;{OTHER})(A;;FR;;;{colleague})"),
    )
    .unwrap();
    let [modify, deny] = fence(&dunce::canonicalize(&matter).unwrap());
    let mut acl = NtfsAcl::new().unwrap();
    acl.add(&modify).unwrap();
    acl.add(&deny).unwrap();

    acl.strip_taken(&place.at("matter"), &[other()], &[], true)
        .unwrap();
    acl.strip_taken(&shared, &[other()], &[], false).unwrap();

    assert!(!protected(&shut), "it inherits again");
    assert!(!has(&shut, OTHER, false));
    assert!(
        has(&shut, OTHER_POOL_SIDS[0], true),
        "the fence's deny reaches it"
    );
    assert!(has(&shut, USER_SID, true), "the matter's Modify reaches it");
    assert_eq!(std::fs::read(&shut).unwrap(), b"x");
    let strict = Strict {
        watched: vec![other()],
        pool: vec![Sid::parse(USER_SID).unwrap(), other()],
        assigned: vec![Sid::parse(USER_SID).unwrap()],
    };
    let walked = acl.propagate(&place.at("matter"), Reach::Tree, Some(&strict));
    assert!(walked.is_ok(), "launchable: {walked:?}");

    let (kept, left) = owner_view(&shared);
    assert!(kept, "a colleague's entry keeps the protection");
    let names: Vec<&str> = left.iter().map(|(_, _, _, s)| s.as_str()).collect();
    assert_eq!(names, [colleague], "only the colleague's entry is left");
}

/// A folder only the agent's user could open (what `--reclaim` took): opened with the owner's
/// implied rights alone, it inherits again, and only then is it listed; the file below it, whose
/// entries were inherited copies of the agent's DACL, inherits the matter's entries too (two
/// writes: the file is written only for its stale copy). No entry for that pool user is left on
/// either. Before the strip neither can be read here (nothing lets the lawyer list the folder),
/// so nothing is read until it ran; [`Unshut`] lets the clean-up remove them if it fails.
#[test]
fn a_shut_folder_inherits_and_then_its_contents_are_handed_back() {
    drop_privileges();
    let place = Place::new("acl-strip-folder");
    let me = current_user_sid().unwrap();
    let matter = dunce::canonicalize(place.dir.dir("matter")).unwrap();
    let work = dunce::canonicalize(place.dir.dir("matter/work")).unwrap();
    let inner = dunce::canonicalize(place.dir.file("matter/work/inner.txt", "x")).unwrap();
    protect_with(&work, &format!("D:P(A;OICI;FA;;;{OTHER})")).unwrap();
    let _unshut = Unshut(&work);
    let [modify, deny] = fence(&matter);
    let mut acl = NtfsAcl::new().unwrap();
    acl.add(&modify).unwrap();
    acl.add(&deny).unwrap();

    let walked = acl.strip_taken(&matter, &[other()], &[], true).unwrap();

    assert_eq!(walked.repaired, 2, "{walked:?}");
    for item in [&work, &inner] {
        assert_eq!(owner(item).unwrap(), me);
        assert!(!protected(item));
        assert!(
            !has(item, OTHER, false),
            "no allow of its own for the pool user"
        );
        assert!(has(item, USER_SID, true), "the matter's Modify reaches it");
        assert!(has(item, OTHER, true), "the fence's deny reaches it");
    }
    assert!(
        !entries(&inner)
            .iter()
            .any(|(k, _, _, s)| *k == 0 && s.as_str() == OTHER),
        "no allow for the pool user at all"
    );
    assert_eq!(std::fs::read(&inner).unwrap(), b"x");
}

/// Lets `work` (and what is below it) inherit again when dropped, as its owner, so the temp
/// folder's clean-up can remove it after a failed strip.
struct Unshut<'a>(&'a std::path::Path);

impl Drop for Unshut<'_> {
    fn drop(&mut self) {
        drop(auto_inherit_with(self.0, "D:"));
    }
}
