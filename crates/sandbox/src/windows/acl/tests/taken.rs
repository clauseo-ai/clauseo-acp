//! S15 `PR4i` on a real NTFS volume: the hand-back after `--reclaim` (`strip_taken`) takes off a
//! lawyer-owned item only the explicit allows of pool users other than the matter's own, and, on
//! an item `--reclaim` took, the denies of the lawyer, its token's groups and the owner's
//! stand-ins: a broad group's allow, the matter's own pool user's, the lawyer's, a kept SID's
//! deny and the DACL's protection stay, and so does an allow the ledger lists as ours (`keep`).

use super::*;
use crate::test_support::{remove, rename};
use crate::windows::acl::object::Object;
use crate::windows::acl_bytes::DENIED;
use crate::windows::test_acl::{entries, owner_view};
use crate::windows::writer::{FileId, TakenIndex};

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
        .strip_taken(
            &place.at("matter"),
            &[other()],
            &[],
            true,
            &DenyStrip::none(),
        )
        .unwrap();

    assert_eq!(walked.repaired, 1);
    assert_eq!(owner(&item).unwrap(), me);
    assert!(protected(&item), "its protection stays");
    assert!(!has(&item, OTHER, false), "another pool user's allow goes");
    for kept in [me.as_str(), "S-1-1-0", USER_SID] {
        assert!(has(&item, kept, false), "{kept} stays");
    }
}

/// `path`'s final path and file ID, as `--reclaim` reports an item it took.
fn taken(path: &Path) -> (PathBuf, FileId) {
    let object = Object::open(path).unwrap();
    let (volume, id) = object.file_id().unwrap();
    let at = PathBuf::from(OsString::from_wide(&object.normalized().unwrap()));
    (at, FileId { volume, id })
}

/// On the items `--reclaim` took, the agent's denies of the lawyer (`WRITE_OWNER`, which the
/// label write needs) and of `Everyone` go, read with the lawyer's real token, on the same
/// limited handle the hand-back opens; a pool user's deny stays, and so does a kept SID's (the
/// pool group's place), even one in the lawyer's token (`Users`). Kept too: the denies of the
/// lawyer's own file and of the matter root (not taken), of a file listed at its path with
/// another file's ID, of a file whose ID is listed at the path it was moved from, and of a taken
/// file with a second hard link (skipped). `OWNER RIGHTS` is the exe's to take off
/// (`setup/reclaim_tests.rs`), since with one the lawyer couldn't open the item at all.
#[test]
fn only_taken_items_lose_the_lawyers_denies_and_kept_ones_stay() {
    drop_privileges();
    let place = Place::new("acl-strip-deny");
    let me = current_user_sid().unwrap();
    let group = "S-1-5-21-9-9-9-1600";
    let shut = dunce::canonicalize(place.dir.file("matter/denied.txt", "x")).unwrap();
    let mine = dunce::canonicalize(place.dir.file("matter/mine.txt", "x")).unwrap();
    let other_id = dunce::canonicalize(place.dir.file("matter/other-id.txt", "x")).unwrap();
    let moved = dunce::canonicalize(place.dir.file("matter/before.txt", "x")).unwrap();
    let linked = dunce::canonicalize(place.dir.file("matter/linked.txt", "x")).unwrap();
    place.dir.hard_link("elsewhere/link.txt", &linked);
    let root = dunce::canonicalize(place.at("matter")).unwrap();
    let (moved_from, moved_id) = taken(&moved);
    let listed = TakenIndex::new([
        taken(&shut),
        (taken(&other_id).0, taken(&shut).1),
        (moved_from, moved_id),
        taken(&linked),
    ]);
    let moved_to = moved.with_file_name("after.txt");
    rename(&moved, &moved_to);
    let dacl = format!(
        "D:P(D;;WO;;;{me})(D;;FA;;;WD)(D;;FA;;;{OTHER})(D;;SD;;;BU)(D;;SD;;;{group})\
         (A;;FA;;;{me})(A;;FA;;;{OTHER})"
    );
    // The root first: setting its DACL by name carries its inherited entries down.
    let left_alone = [&root, &mine, &other_id, &moved_to, &linked];
    for path in left_alone {
        auto_inherit_with(path, "D:(D;;SD;;;WD)").unwrap();
    }
    protect_with(&shut, &dacl).unwrap();
    let kept = [
        other(),
        Sid::parse("S-1-5-32-545").unwrap(),
        Sid::parse(group).unwrap(),
    ];
    let denies = DenyStrip {
        taken: &listed,
        kept: &kept,
    };

    NtfsAcl::new()
        .unwrap()
        .strip_taken(&root, &[other()], &[], true, &denies)
        .unwrap();

    let named = |path: &Path, kind: u8, sid: &str| {
        entries(path)
            .iter()
            .any(|e| e.0 == kind && e.3.as_str() == sid)
    };
    assert!(
        !named(&shut, DENIED, me.as_str()),
        "the deny of the lawyer goes"
    );
    assert!(
        !named(&shut, DENIED, "S-1-1-0"),
        "the deny of Everyone goes"
    );
    for stays in [OTHER, "S-1-5-32-545", group] {
        assert!(named(&shut, DENIED, stays), "{stays}'s deny stays");
    }
    assert!(
        !named(&shut, ALLOWED, OTHER),
        "another pool user's allow goes"
    );
    assert!(
        named(&shut, ALLOWED, me.as_str()),
        "the lawyer's allow stays"
    );
    for path in left_alone {
        assert!(
            named(path, DENIED, "S-1-1-0"),
            "not taken: {} keeps its deny",
            path.display()
        );
    }
}

/// The file ID is read on the item's own handle, so nothing else can lend one: a lawyer's own
/// file put in under a taken file's name, after that file went, keeps its denies (it isn't the
/// file taken). A taken file listed under its name in another case is still the one taken, as
/// NTFS ignores case: its denies go, and it isn't refused.
#[test]
fn a_file_swapped_in_under_a_taken_name_keeps_its_denies_and_case_is_ignored() {
    drop_privileges();
    let place = Place::new("acl-strip-swap");
    let me = current_user_sid().unwrap();
    let gone = dunce::canonicalize(place.dir.file("matter/swapped.txt", "x")).unwrap();
    let cased = dunce::canonicalize(place.dir.file("matter/cased.txt", "x")).unwrap();
    let root = dunce::canonicalize(place.at("matter")).unwrap();
    let (swapped_at, swapped_id) = taken(&gone);
    let (cased_at, cased_id) = taken(&cased);
    remove(&gone);
    let swapped = dunce::canonicalize(place.dir.file("matter/swapped.txt", "mine")).unwrap();
    let upper = cased_at.with_file_name("CASED.TXT");
    let listed = TakenIndex::new([(swapped_at, swapped_id), (upper, cased_id)]);
    for path in [&swapped, &cased] {
        protect_with(path, &format!("D:P(D;;SD;;;WD)(A;;FA;;;{me})")).unwrap();
    }
    let denies = DenyStrip {
        taken: &listed,
        kept: &[other()],
    };

    NtfsAcl::new()
        .unwrap()
        .strip_taken(&root, &[other()], &[], true, &denies)
        .unwrap();

    let denied = |path: &Path| {
        entries(path)
            .iter()
            .any(|e| e.0 == DENIED && e.3.as_str() == "S-1-1-0")
    };
    assert!(
        denied(&swapped),
        "another file under a taken name keeps its deny"
    );
    assert!(
        !denied(&cased),
        "the taken file, listed in another case, loses it"
    );
}

/// Below a folder `--reclaim` took, whose agent denied the lawyer `WRITE_OWNER` for everything
/// in it, a file the lawyer owns holds an inherited copy of that deny, which nothing propagates
/// away. Once the folder's deny goes, the file inherits again: the copy goes too, and its own
/// explicit deny, which nobody took, stays.
#[test]
fn below_a_taken_folder_the_copies_of_its_denies_go() {
    drop_privileges();
    let place = Place::new("acl-strip-below");
    let me = current_user_sid().unwrap();
    let below = dunce::canonicalize(place.dir.file("matter/work/below.txt", "x")).unwrap();
    let work = dunce::canonicalize(place.at("matter/work")).unwrap();
    let root = dunce::canonicalize(place.at("matter")).unwrap();
    auto_inherit_with(&below, "D:(D;;SD;;;WD)").unwrap();
    // Set by name, so the copy reaches the file below.
    protect_with(&work, &format!("D:P(D;OICI;WO;;;{me})(A;OICI;FA;;;{me})")).unwrap();
    assert!(
        has(&below, me.as_str(), true),
        "the copy is there to start with"
    );
    let listed = TakenIndex::new([taken(&work)]);
    let denies = DenyStrip {
        taken: &listed,
        kept: &[other()],
    };

    NtfsAcl::new()
        .unwrap()
        .strip_taken(&root, &[other()], &[], true, &denies)
        .unwrap();

    let denied = |path: &Path, sid: &str, inherited: bool| {
        entries(path)
            .iter()
            .any(|e| e.0 == DENIED && e.3.as_str() == sid && (e.1 & INHERITED != 0) == inherited)
    };
    assert!(
        !denied(&work, me.as_str(), false),
        "the taken folder's deny goes"
    );
    assert!(!denied(&below, me.as_str(), true), "and its copy below");
    assert!(
        denied(&below, "S-1-1-0", false),
        "the file's own deny stays"
    );
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

    acl.strip_taken(&root, &[other()], &[], false, &DenyStrip::none())
        .unwrap();

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
    acl.strip_taken(
        &place.at("state"),
        &[other()],
        &keep,
        true,
        &DenyStrip::none(),
    )
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
            &DenyStrip::none(),
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

    acl.strip_taken(
        &place.at("matter"),
        &[other()],
        &[],
        true,
        &DenyStrip::none(),
    )
    .unwrap();
    acl.strip_taken(&shared, &[other()], &[], false, &DenyStrip::none())
        .unwrap();

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

    let walked = acl
        .strip_taken(&matter, &[other()], &[], true, &DenyStrip::none())
        .unwrap();

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
