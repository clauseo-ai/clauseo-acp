//! The S15 `PR3b` round 5 fixes in the apply layer: the credential link's entries removed from
//! the credentials file by its ID, and a shut child past `MAX_PATH` matched to its root.

use super::round4_ledger::{launch_through_link, on_file};
use super::*;
use crate::windows::acl_plan::verbatim;
use crate::windows::apply::walk::clearing;
use crate::windows::writer::{Reach, Strict};

/// The lawyer's own label on the credentials file before the launch.
const LAWYER: Label = Label {
    level: 8192,
    flags: 0,
    mask: 0x1,
};

/// Round 5, issue 1: after a launch the credentials file is renamed and another file put at its
/// path. Removal went by the path, found another file ID there and refused `Changed`, so the
/// entries stayed `removing` on the original. It now opens the original by its ID: its entries
/// and Low label come off, its own label goes back, and the new file is left alone.
#[test]
fn forget_takes_the_entries_off_the_renamed_credentials_file_and_leaves_the_new_one_alone() {
    let w = world("r5-renamed", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    w.dir.file("elsewhere/auth.json", "{}");
    acl.labels
        .insert(w.canonical("elsewhere/auth.json"), LAWYER);
    let (auth, _) = launch_through_link(&w, &mut ledger, &mut acl);
    w.dir.file("elsewhere/fresh.json", "{}");
    let fresh = w.canonical("elsewhere/fresh.json");
    let theirs = Label {
        level: 12288,
        ..LAWYER
    };
    acl.links.insert(auth.clone(), fresh.clone());
    acl.labels.insert(fresh.clone(), theirs);
    let ace = (
        fresh.clone(),
        USER_SID.to_owned(),
        Rights::Modify,
        Inherit::ThisOnly,
    );
    acl.aces.insert(ace.clone());
    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(done.kept, 0, "{done:?}");
    assert_eq!(on_file(&acl, &auth), 0, "{:?}", acl.aces);
    assert_eq!(
        acl.labels.get(&auth),
        Some(&LAWYER),
        "the lawyer's label is back"
    );
    assert_eq!(on_file(&acl, &fresh), 1, "{:?}", acl.aces);
    assert!(acl.aces.contains(&ace));
    assert_eq!(acl.labels.get(&fresh), Some(&theirs));
    assert_eq!(ledger.entries().count(), 0);
}

/// Round 5, issue 1: the credentials file's own name is deleted, but the file lives on as the
/// credential link. Removal went by the path, found nothing, and dropped the entries while the
/// file kept the grant through the link. It now finds the file by its ID and takes them off.
#[test]
fn forget_takes_the_entries_off_a_credentials_file_whose_name_was_deleted() {
    let w = world("r5-unnamed", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let (auth, _) = launch_through_link(&w, &mut ledger, &mut acl);
    acl.gone.insert(auth.clone());
    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(done.kept, 0, "{done:?}");
    assert_eq!(on_file(&acl, &auth), 0, "{:?}", acl.aces);
    assert_eq!(ledger.entries().count(), 0);
}

/// Round 5, issue 2: `clearing` compared in `dunce`'s plain form, which keeps a path past
/// `MAX_PATH` verbatim, so a shut child with a long path matched no root and every launch was
/// refused. Both sides are now verbatim whatever their length: a long child is matched to its
/// root and set aside, the child verbatim and the root plain, or the other way round.
#[test]
fn a_shut_child_past_max_path_is_matched_to_its_root_and_set_aside() {
    let w = world("r5-long", &[]);
    let matter = w.canonical("matter");
    let child = matter
        .join("a".repeat(130))
        .join("b".repeat(130))
        .join("shut.txt");
    assert!(child.as_os_str().len() > 260);
    for (prefix, root) in [(r"\\?\", matter.clone()), ("", verbatim(&matter))] {
        let mut acl = FakeAcl {
            prefix,
            shut: vec![child.clone()],
            ..FakeAcl::default()
        };
        let mut around = around(&w);
        let strict = Strict::default();
        let walked = clearing(
            &mut acl,
            &mut around,
            &[(root, matter_one())],
            PathKind::Matter,
            |acl| acl.propagate(&matter, Reach::Tree, Some(&strict)),
        );
        assert!(walked.is_ok(), "{prefix:?}: {walked:?}");
        let reported = PathBuf::from(format!("{prefix}{}", child.display()));
        assert_eq!(around.set_aside, vec![reported]);
    }
}

/// Round 5, issue 2: every form of a path becomes one verbatim form, whatever its length.
#[test]
fn every_form_of_a_path_becomes_one_verbatim_form() {
    let long = format!(r"C:\m\{}", "x".repeat(300));
    let cases = [
        (r"C:\m".to_owned(), r"\\?\C:\m".to_owned()),
        (r"\\?\C:\m".to_owned(), r"\\?\C:\m".to_owned()),
        (r"\\.\C:\m".to_owned(), r"\\?\C:\m".to_owned()),
        (
            r"\\server\share\m".to_owned(),
            r"\\?\UNC\server\share\m".to_owned(),
        ),
        (
            r"\\?\UNC\server\share".to_owned(),
            r"\\?\UNC\server\share".to_owned(),
        ),
        (
            r"\\.\UNC\server\share".to_owned(),
            r"\\?\UNC\server\share".to_owned(),
        ),
        (long.clone(), format!(r"\\?\{long}")),
        (format!(r"\\?\{long}"), format!(r"\\?\{long}")),
    ];
    for (given, want) in cases {
        assert_eq!(verbatim(Path::new(&given)), PathBuf::from(want), "{given}");
    }
}
