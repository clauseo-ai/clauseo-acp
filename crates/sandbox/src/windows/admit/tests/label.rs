//! The folders the plan labels need `WRITE_OWNER` too: a grant folder the lawyer can't label is
//! refused as `CantLabel` before anything is planned; a folder only denied isn't asked.

use super::*;

/// The gate with `label` for the labelled folders and `Ok` for the rest, then the plan and the
/// ledger's pending entries, as [`prepare`] does.
fn prepare_labelling(
    w: &World,
    policy: &Policy,
    label: impl FnMut(&Path) -> io::Result<FolderSupport>,
    folders: &FakeFolders,
    ledger: &mut Ledger,
) -> Result<usize, Error> {
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = Neighbours::default();
    let admitted = admit_labelling_with(policy, &w.at("data"), &none, ok, label, folders)?;
    let mut plan = plan(&admitted, &sids(), matter_one())?;
    ledger.record_pending(&mut plan).map_err(Error::Probe)?;
    Ok(plan.entries().len())
}

#[test]
fn a_labelled_folder_without_write_owner_is_refused_as_cant_label() {
    let w = World::new("admit-cant-label");
    let policy = w.full_policy();
    let cases = [
        ("matter", PathKind::Matter),
        (STATE, PathKind::State),
        (NOTES, PathKind::Notes),
        (STORE, PathKind::ClaudeStore),
    ];
    let (mut ledger, before) = ledger_with_an_entry(&w);
    for (rel, kind) in cases {
        let label = answering(w.canonical(rel), || Ok(FolderSupport::CantLabel));
        let folders = FakeFolders::default();
        let refused = prepare_labelling(&w, &policy, label, &folders, &mut ledger);
        let Err(error @ Error::FolderUnsupported { path_kind, support }) = &refused else {
            panic!("not refused: {refused:?}");
        };
        let want = FolderRefusal::Support(FolderSupport::CantLabel);
        assert_eq!((*path_kind, *support), (kind, want));
        // Refused at the gate: no plan, no pending entry.
        assert_eq!(std::fs::read(ledger.path()).unwrap(), before, "{rel}");
        let text = format!("{error} {error:?}");
        assert!(text.contains("label"), "{text}");
        let root = dunce::canonicalize(w.dir.path()).unwrap();
        assert!(!text.contains(&*root.to_string_lossy()), "{text}");
    }
}

#[test]
fn a_labelled_folder_with_both_rights_is_admitted() {
    let w = World::new("admit-can-label");
    let policy = w.full_policy();
    let mut asked = Vec::new();
    let label = |path: &Path| {
        asked.push(path.to_owned());
        Ok(FolderSupport::Ok)
    };
    let (mut ledger, _) = ledger_with_an_entry(&w);
    let planned = prepare_labelling(&w, &policy, label, &FakeFolders::default(), &mut ledger);
    assert!(planned.unwrap() > 0);
    let want = [
        w.canonical("matter"),
        w.canonical(STATE),
        w.canonical(NOTES),
        w.canonical(STORE),
    ];
    assert_eq!(asked, want);
}

#[test]
fn a_folder_only_denied_needs_no_write_owner() {
    // A sibling gets the pool group's deny, not the label: the label check would refuse it, and
    // it isn't asked; the entries check admits it.
    let w = World::new("admit-sibling-no-label");
    w.dir.dir("Drafts");
    let drafts = w.canonical("Drafts");
    let policy = w.builder().build().unwrap();
    let folders = FakeFolders {
        listing: vec![drafts.clone()],
        ..FakeFolders::default()
    };
    let label = answering(drafts.clone(), || Ok(FolderSupport::CantLabel));
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let none = Neighbours::default();
    let admitted =
        admit_labelling_with(&policy, &w.at("data"), &none, ok, label, &folders).unwrap();
    assert_eq!(admitted.siblings(), [drafts]);
}
