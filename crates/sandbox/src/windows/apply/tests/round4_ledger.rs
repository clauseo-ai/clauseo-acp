//! The S15 `PR3b` round 4 fixes in the ledger's way back, from the second VM run: the credential
//! link's entries taken back from the credentials file it recorded (row 5, L5).

use super::*;
use crate::windows::writer::FileId;

/// A launch of matter one whose credential link is a second name for `elsewhere/auth.json`;
/// returns the credentials file and the number of the link's entries.
fn launch_through_link(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl) -> (PathBuf, usize) {
    w.dir.file("elsewhere/auth.json", "{}");
    w.dir.file("elsewhere/other.json", "{}");
    let auth = w.canonical("elsewhere/auth.json");
    acl.links
        .insert(w.canonical(STATE).join("auth.json"), auth.clone());
    let mut plan = launch_plan(w, &["sib1"]);
    apply(ledger, &mut plan, acl, &mut around(w)).unwrap();
    let on_link = |e: &&crate::windows::ledger::Entry| e.kind == PathKind::CredentialLink;
    let entries = ledger.entries().filter(on_link).count();
    assert!(entries >= 2, "the link's grant and its Low label");
    assert_eq!(on_file(acl, &auth), entries);
    (auth, entries)
}

/// How many permission entries `file` holds.
fn on_file(acl: &FakeAcl, file: &Path) -> usize {
    acl.aces.iter().filter(|(p, _, _, _)| p == file).count()
}

/// Round 4, row 5 (L5): after a launch, the agent swaps the credential link for a hard link to
/// another file. Forget took the link's entries back through the link's path, so from the other
/// file, and left the grant and the Low label on `auth.json`. They now come off the recorded
/// credentials file.
#[test]
fn forget_takes_the_links_entries_off_the_credentials_file_after_the_link_is_swapped() {
    let w = world("r4-l5-swap", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let (auth, entries) = launch_through_link(&w, &mut ledger, &mut acl);
    let other = w.canonical("elsewhere/other.json");
    acl.links
        .insert(w.canonical(STATE).join("auth.json"), other.clone());
    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(done.kept, 0, "{done:?}");
    assert_eq!(on_file(&acl, &auth), 0, "{:?}", acl.aces);
    assert_eq!(on_file(&acl, &other), 0);
    assert!(done.removed >= entries);
    assert_eq!(ledger.entries().count(), 0);
}

/// Round 4, row 5 (L5): another file at the credentials file's recorded path (its ID differs)
/// is never written: the link's entries stay, not applied, and forget reports them kept.
#[test]
fn another_file_at_the_credentials_path_keeps_the_links_entries_and_reports_them() {
    let w = world("r4-l5-mismatch", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let (auth, entries) = launch_through_link(&w, &mut ledger, &mut acl);
    let stranger = FileId {
        volume: 1,
        id: [7; 16],
    };
    acl.ids.insert(auth.clone(), stranger);
    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(done.kept, entries, "{done:?}");
    assert_eq!(
        on_file(&acl, &auth),
        entries,
        "nothing written on another file"
    );
    let left: Vec<_> = ledger.entries().collect();
    assert_eq!(left.len(), entries, "{left:?}");
    assert!(
        left.iter()
            .all(|e| e.kind == PathKind::CredentialLink && e.state != EntryState::Applied)
    );
}

/// Round 4, row 5 (L5): a credentials file that is gone took the link's entries with it, so
/// forget drops them, as it does an entry whose path is missing.
#[test]
fn a_credentials_file_that_is_gone_lets_forget_drop_the_links_entries() {
    let w = world("r4-l5-gone", &[]);
    let (mut ledger, mut acl) = (ledger(&w), FakeAcl::default());
    let (auth, _) = launch_through_link(&w, &mut ledger, &mut acl);
    acl.gone.insert(auth);
    let done = forget_matter(&mut ledger, matter_one(), &mut acl, &mut around(&w)).unwrap();
    assert_eq!(done.kept, 0, "{done:?}");
    assert_eq!(ledger.entries().count(), 0);
}
