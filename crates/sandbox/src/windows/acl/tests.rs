//! The permission writes and the walk on a real NTFS volume: temp folders and synthetic SIDs, no
//! accounts. Windows CI and win-01 run these.

use super::*;
use crate::windows::acl_plan::{LOW_LABEL_SID, Rights};
use crate::windows::test_acl::{Place, grant, has, protected};
use crate::windows::test_dacl::protect_with;
use crate::windows::test_junction::junction;
use crate::windows::test_world::{OTHER_POOL_SIDS, USER_SID};

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
