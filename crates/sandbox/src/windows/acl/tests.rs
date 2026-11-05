//! The permission writes and the walk on a real NTFS volume: temp folders and synthetic SIDs, no
//! accounts. Windows CI and win-01 run these.

use super::*;
use crate::windows::acl_plan::{LOW_LABEL_SID, Rights};
use crate::windows::test_acl::{Place, grant, has, protected};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_junction::junction;
use crate::windows::test_world::{OTHER_POOL_SIDS, USER_SID};
use crate::windows::writer::Label;
use walk::race::{HOOK, REFUSE, Stage};

/// The pool user's `Modify` and another pool user's deny, both down the tree.
fn fence(matter: &Path) -> [Grant; 2] {
    [
        grant(matter, USER_SID, Rights::Modify, Inherit::Tree),
        grant(
            matter,
            OTHER_POOL_SIDS[0],
            Rights::DenyReadWrite,
            Inherit::Tree,
        ),
    ]
}

/// Writes `grants` and walks the tree strictly, as a launch does.
fn put(root: &Path, grants: &[Grant]) -> Result<Walked, ApplyFailure> {
    let mut acl = NtfsAcl::new().unwrap();
    for g in grants {
        acl.add(g)?;
    }
    let watched = [Sid::parse(OTHER_POOL_SIDS[0]).unwrap()];
    acl.propagate(root, Reach::Tree, Some(&watched))
}

#[test]
fn junction_and_hard_link_targets_stay_bare() {
    let place = Place::new("acl-links");
    place.dir.file("matter/sub/doc.txt", "mine");
    place.dir.file("out/secret.txt", "outside");
    let original = place.dir.file("out2/h.txt", "linked");
    place.dir.hard_link("matter/h.txt", &original);
    junction(&place.at("matter/j"), &place.at("out")).unwrap();

    let walked = put(&place.at("matter"), &fence(&place.at("matter"))).unwrap();
    assert!(walked.skipped >= 2, "{walked:?}");
    assert!(has(&place.at("matter/sub/doc.txt"), USER_SID, true));
    assert!(has(
        &place.at("matter/sub/doc.txt"),
        OTHER_POOL_SIDS[0],
        true
    ));
    for bare in ["out/secret.txt", "out2/h.txt", "out"] {
        assert!(!has(&place.at(bare), USER_SID, true), "{bare}");
        assert!(!has(&place.at(bare), OTHER_POOL_SIDS[0], true), "{bare}");
    }
}

#[test]
fn three_hindi_spellings_get_the_entry_on_the_final_path_and_a_sibling_stays_bare() {
    let place = Place::new("acl-hindi");
    // क़ानून with the nukta as its own mark; the sibling uses the precomposed क़ (U+0958),
    // another name to NTFS.
    let name = "\u{915}\u{93c}\u{93e}\u{928}\u{942}\u{928}";
    let sibling = "\u{958}\u{93e}\u{928}\u{942}\u{928}";
    let canonical = dunce::canonicalize(place.dir.dir(name)).unwrap();
    place.dir.dir(sibling);
    let text = canonical.to_string_lossy().into_owned();
    let forms = [
        canonical.clone(),
        PathBuf::from(format!(r"\\?\{text}")),
        PathBuf::from(text.to_ascii_uppercase()),
    ];
    let mut acl = NtfsAcl::new().unwrap();
    for (i, form) in forms.iter().enumerate() {
        let modify = grant(form, USER_SID, Rights::Modify, Inherit::ThisOnly);
        let added = acl.add(&modify).unwrap();
        let want = if i == 0 {
            Added::Written
        } else {
            Added::AlreadyThere
        };
        assert_eq!(added, want, "{}", form.display());
        assert!(has(&canonical, USER_SID, false), "{}", form.display());
    }
    assert!(!has(&place.at(sibling), USER_SID, false));
    assert!(!has(&place.at(sibling), USER_SID, true));
}

#[test]
fn a_target_reached_through_a_junction_is_refused() {
    let place = Place::new("acl-changed");
    place.dir.dir("matter");
    junction(&place.at("alias"), &place.at("matter")).unwrap();
    let through = grant(&place.at("alias"), USER_SID, Rights::Modify, Inherit::Tree);
    let mut acl = NtfsAcl::new().unwrap();
    assert_eq!(acl.add(&through), Err(ApplyFailure::Changed));
    assert!(!has(&place.at("matter"), USER_SID, false));
}

#[test]
fn a_whole_tree_deny_on_a_drive_root_is_refused() {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let root = PathBuf::from(format!("{drive}\\"));
    let deny = grant(&root, OTHER_POOL_SIDS[0], Rights::DenyRead, Inherit::Tree);
    assert_eq!(
        NtfsAcl::new().unwrap().add(&deny),
        Err(ApplyFailure::VolumeRoot)
    );
}

#[test]
fn a_protected_subfolder_fails_the_strict_walk() {
    let place = Place::new("acl-protected");
    place.dir.file("matter/sub/doc.txt", "x");
    protect_with(&place.at("matter/sub"), "D:P(A;OICI;FA;;;OW)").unwrap();
    let walked = put(&place.at("matter"), &fence(&place.at("matter")));
    assert_eq!(walked, Err(ApplyFailure::ProtectedChild));
}

#[test]
fn a_child_with_an_explicit_everyone_allow_fails_the_strict_walk() {
    let place = Place::new("acl-everyone");
    place.dir.file("matter/open/doc.txt", "x");
    let everyone = grant(
        &place.at("matter/open"),
        "S-1-1-0",
        Rights::ReadAndExecute,
        Inherit::Tree,
    );
    NtfsAcl::new().unwrap().add(&everyone).unwrap();
    let walked = put(&place.at("matter"), &fence(&place.at("matter")));
    assert_eq!(walked, Err(ApplyFailure::BroadAllowChild));
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "moves a test file into the matter"
)]
fn the_next_walk_reaches_a_file_moved_in_since() {
    let place = Place::new("acl-moved");
    place.dir.dir("matter");
    put(&place.at("matter"), &fence(&place.at("matter"))).unwrap();
    let outside = place.dir.file("outside/late.txt", "moved in");
    let inside = place.at("matter/late.txt");
    std::fs::rename(&outside, &inside).unwrap();
    assert!(
        !has(&inside, USER_SID, true),
        "a move keeps the old entries"
    );

    put(&place.at("matter"), &fence(&place.at("matter"))).unwrap();
    assert!(has(&inside, USER_SID, true));
    assert!(has(&inside, OTHER_POOL_SIDS[0], true));
}

/// Unverified behaviour 1 and 3: a walk after a removal drops the inherited copies (DACL and
/// label), and a label SACL with no label removes the label.
#[test]
fn removing_an_entry_and_walking_again_takes_the_inherited_copies_back() {
    let place = Place::new("acl-label");
    let doc = place.dir.file("matter/sub/doc.txt", "x");
    let matter = place.at("matter");
    let label = grant(&matter, LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree);
    let mut grants = fence(&matter).to_vec();
    grants.push(label);
    put(&matter, &grants).unwrap();
    assert!(has(&doc, LOW_LABEL_SID, true));
    assert!(has(&matter, LOW_LABEL_SID, false));
    assert!(has(&doc, USER_SID, true));

    let mut acl = NtfsAcl::new().unwrap();
    for g in &grants {
        acl.remove(g).unwrap();
        assert!(!acl.present(g).unwrap());
    }
    acl.propagate(&matter, Reach::Tree, None).unwrap();
    for path in [&matter, &doc, &place.at("matter/sub")] {
        for sid in [USER_SID, OTHER_POOL_SIDS[0], LOW_LABEL_SID] {
            assert!(
                !has(path, sid, true) && !has(path, sid, false),
                "{sid} {path:?}"
            );
        }
    }
}

/// Unverified behaviour 2: an entry written on a protected folder keeps it protected, and its
/// children's walk keeps theirs.
#[test]
fn a_protected_target_stays_protected() {
    let place = Place::new("acl-keeps-protection");
    place.dir.file("matter/doc.txt", "x");
    protect_with(&place.at("matter"), "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)").unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    acl.add(&fence(&place.at("matter"))[0]).unwrap();
    acl.propagate(&place.at("matter"), Reach::Tree, None)
        .unwrap();
    assert!(protected(&place.at("matter")));
    assert!(has(&place.at("matter"), USER_SID, false));
    assert!(has(&place.at("matter/doc.txt"), USER_SID, true));
    acl.remove(&fence(&place.at("matter"))[0]).unwrap();
    assert!(protected(&place.at("matter")));
}

/// Sets the walk's race hook to run `swap` once, at `stage` for the folder named `folder`.
fn race_at(stage: Stage, folder: &'static str, swap: impl FnOnce() + 'static) {
    let mut swap = Some(swap);
    HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |at: Stage, path: &Path| {
            let named = path.file_name().is_some_and(|name| name == folder);
            if at == stage
                && named
                && let Some(swap) = swap.take()
            {
                swap();
            }
        }));
    });
}

fn clear_race() {
    HOOK.with(|hook| *hook.borrow_mut() = None);
}

/// H1: an agent swaps `matter\a` for a junction to a folder outside after the walk listed the
/// matter. The walk opens the name relative to the matter's handle, so it gets the junction
/// itself, skips it, and nothing outside gets the pool user's entries.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test plays the agent's rename"
)]
fn a_subfolder_swapped_for_a_junction_after_the_listing_is_never_passed_through() {
    let place = Place::new("acl-race-listed");
    place.dir.file("matter/a/doc.txt", "mine");
    let secret = place.dir.file("out/secret.txt", "outside");
    let (a, aside, out) = (place.at("matter/a"), place.at("matter-a"), place.at("out"));
    race_at(Stage::Listed, "matter", move || {
        std::fs::rename(&a, &aside).unwrap();
        junction(&a, &out).unwrap();
    });
    let walked = put(&place.at("matter"), &fence(&place.at("matter")));
    clear_race();
    let walked = walked.unwrap();
    assert!(walked.skipped >= 1, "{walked:?}");
    for bare in [secret.as_path(), &place.at("out")] {
        assert!(!has(bare, USER_SID, true), "{bare:?}");
        assert!(!has(bare, OTHER_POOL_SIDS[0], true), "{bare:?}");
    }
}

/// H1: an agent moves `matter\a` away after the walk opened it, and puts a junction in its
/// place. The folder's final path is checked again before it is listed: the walk refuses, and
/// nothing outside gets the pool user's entries.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test plays the agent's rename"
)]
fn a_subfolder_moved_after_its_check_refuses_the_walk() {
    let place = Place::new("acl-race-listing");
    place.dir.file("matter/a/doc.txt", "mine");
    let secret = place.dir.file("out/secret.txt", "outside");
    let (a, aside, out) = (place.at("matter/a"), place.at("matter-a"), place.at("out"));
    race_at(Stage::Listing, "a", move || {
        std::fs::rename(&a, &aside).unwrap();
        junction(&a, &out).unwrap();
    });
    let walked = put(&place.at("matter"), &fence(&place.at("matter")));
    clear_race();
    assert_eq!(walked, Err(ApplyFailure::Changed));
    assert!(!has(&secret, USER_SID, true));
    assert!(!has(&place.at("matter-a/doc.txt"), USER_SID, true));
}

/// L3: reconcile asks about the planned target; a junction there now (pointing at the folder
/// that holds the entry) is an error, not "missing", so the pending entry isn't dropped.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves the matter away")]
fn presence_through_a_swapped_name_is_an_error() {
    let place = Place::new("acl-present");
    place.dir.dir("matter");
    let modify = grant(&place.at("matter"), USER_SID, Rights::Modify, Inherit::Tree);
    let mut acl = NtfsAcl::new().unwrap();
    acl.add(&modify).unwrap();
    assert!(acl.present(&modify).unwrap());
    std::fs::rename(place.at("matter"), place.at("moved")).unwrap();
    junction(&place.at("matter"), &place.at("moved")).unwrap();
    assert!(acl.present(&modify).is_err());
}

/// L4: the lawyer's own explicit label is read before ours replaces it, and put back.
#[test]
fn the_lawyers_own_label_is_put_back_after_ours() {
    let place = Place::new("acl-own-label");
    let matter = place.dir.dir("matter");
    let matter = dunce::canonicalize(matter).unwrap();
    let low = grant(&matter, LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree);
    let lawyers = Label {
        level: 4096,
        flags: 0,
        mask: 0x3,
    };
    let mut acl = NtfsAcl::new().unwrap();
    assert_eq!(acl.label(&matter).unwrap(), None);
    acl.restore_label(&low, lawyers).unwrap();
    assert_eq!(acl.label(&matter).unwrap(), Some(lawyers));
    assert_eq!(acl.add(&low).unwrap(), Added::Written);
    assert_ne!(acl.label(&matter).unwrap(), Some(lawyers));
    acl.restore_label(&low, lawyers).unwrap();
    assert_eq!(acl.label(&matter).unwrap(), Some(lawyers));
    assert!(!acl.present(&low).unwrap());
}

/// H1 re-check: `a` moved away after it was listed. Its children open through its handle, so
/// their final paths are no longer `matter\a\name`: the walk refuses before writing them.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test plays the agent's rename"
)]
fn a_subfolder_moved_after_its_own_listing_refuses_the_walk() {
    let place = Place::new("acl-race-own-listing");
    place.dir.file("matter/a/doc.txt", "mine");
    let (a, aside) = (place.at("matter/a"), place.at("matter-a"));
    race_at(Stage::Listed, "a", move || {
        std::fs::rename(&a, &aside).unwrap();
    });
    let walked = put(&place.at("matter"), &fence(&place.at("matter")));
    clear_race();
    assert_eq!(walked, Err(ApplyFailure::Changed));
    assert!(!has(&place.at("matter-a/doc.txt"), USER_SID, true));
}

/// A child whose open met a reparse point refuses a strict walk and is skipped by a plain one.
#[test]
fn an_open_refused_for_a_reparse_point_refuses_only_a_strict_walk() {
    let place = Place::new("acl-reparse-refused");
    place.dir.file("matter/a/doc.txt", "mine");
    REFUSE.with(|refuse| *refuse.borrow_mut() = Some("a".into()));
    let strict = put(&place.at("matter"), &fence(&place.at("matter")));
    let plain = NtfsAcl::new()
        .unwrap()
        .propagate(&place.at("matter"), Reach::Tree, None);
    REFUSE.with(|refuse| *refuse.borrow_mut() = None);
    assert_eq!(strict, Err(ApplyFailure::Changed));
    assert!(plain.unwrap().skipped >= 1);
}

/// A walk's root that is a junction now refuses, rather than walk nothing and pass.
#[test]
fn a_root_that_is_a_junction_now_refuses_the_walk() {
    let place = Place::new("acl-root-junction");
    place.dir.file("real/doc.txt", "x");
    junction(&place.at("matter"), &place.at("real")).unwrap();
    let walked = NtfsAcl::new()
        .unwrap()
        .propagate(&place.at("matter"), Reach::Tree, None);
    assert_eq!(walked, Err(ApplyFailure::Changed));
}

/// The Windows run at `fca0bf0ca`: a second walk writes nothing, and the matter root keeps its
/// auto-inherited mark (`AI`) through add, walk and remove.
#[test]
fn a_second_walk_writes_nothing_and_the_root_keeps_auto_inherited() {
    let place = Place::new("acl-auto-inherit");
    place.dir.file("matter/sub/doc.txt", "x");
    let matter = place.at("matter");
    let auto = |path: &Path| {
        let bytes = descriptor(path).unwrap();
        Descriptor::parse(&bytes).unwrap().control & DACL_AUTO_INHERITED != 0
    };
    assert!(
        auto(&matter),
        "a new folder under the temp folder is auto-inherited"
    );
    let grants = fence(&matter);
    put(&matter, &grants).unwrap();
    assert!(auto(&matter));
    assert!(auto(&place.at("matter/sub/doc.txt")));
    let again = NtfsAcl::new()
        .unwrap()
        .propagate(&matter, Reach::Tree, None)
        .unwrap();
    assert_eq!(again.written, 0, "{again:?}");
    let mut acl = NtfsAcl::new().unwrap();
    for g in &grants {
        acl.remove(g).unwrap();
    }
    assert!(auto(&matter));
}
