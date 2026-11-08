//! A read of another matter (R18) is taken back from the item it was written on, found by its
//! file ID wherever it is now: moved, deleted or replaced during the run, at the run's end, a
//! relaunch or start. No read is written into a tree the fence doesn't hold.

use super::fake::Broad;
use super::reads::access::volume_default;
use super::*;
use crate::windows::ledger::Entry;

fn pool() -> Vec<Sid> {
    vec![sid(USER_SID)]
}

/// A fake with the volume's default entry at the top, whose strict walks refuse an allow
/// beating a fence.
fn volume(w: &World) -> FakeAcl {
    let mut acl = FakeAcl::default();
    let top = dunce::canonicalize(w.dir.path()).unwrap();
    acl.aces.insert(volume_default(&top));
    acl.broad = Broad::Refused;
    acl
}

/// Matter one's plan reading each of `reads`, matter two passed as `fenced` or not.
fn read_plan(w: &World, reads: &[(&str, bool)], fenced: bool) -> AdmittedPlan {
    let policy = w.reading(reads);
    let ok = |_: &Path| Ok(FolderSupport::Ok);
    let mut others = w.neighbour_m2();
    others.registered[0].fenced = fenced;
    let admitted = admit_with(&policy, &w.at("data"), &others, ok, &FakeFolders::default());
    plan(&admitted.unwrap(), &sids(), matter_one()).unwrap()
}

fn launch(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl, reads: &[(&str, bool)]) {
    let mut plan = read_plan(w, reads, false);
    apply(ledger, &mut plan, acl, &mut around(w)).unwrap();
}

fn read_entries(ledger: &Ledger) -> Vec<Entry> {
    let reads = ledger
        .entries()
        .filter(|e| e.kind == PathKind::OtherMatterRead);
    reads.cloned().collect()
}

/// Every allow for matter one's pool user under `other`.
fn allows_in_other(w: &World, acl: &FakeAcl) -> Vec<Key> {
    let other = w.canonical("other");
    let aces = acl.aces.iter();
    aces.filter(|(p, s, r, _)| p.starts_with(&other) && s == USER_SID && r.is_allow())
        .cloned()
        .collect()
}

/// `docs` moved on disk and in the fake to `to`, relative to the temp folder.
#[expect(
    clippy::disallowed_methods,
    reason = "the other matter's agent or the lawyer moves the folder, not the sandbox"
)]
fn move_docs(w: &World, acl: &mut FakeAcl, to: &str) -> PathBuf {
    let docs = w.canonical("other/m2/docs");
    std::fs::rename(&docs, w.at(to)).unwrap();
    let to = w.canonical(to);
    acl.move_item(&docs, &to);
    to
}

fn end(w: &World, ledger: &mut Ledger, acl: &mut FakeAcl) -> Narrowed {
    end_reads(ledger, matter_one(), &pool(), acl, &mut around(w)).unwrap()
}

#[test]
fn every_read_entry_records_the_file_id_of_the_item_it_is_written_on() {
    let w = World::new("files-recorded");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let reads = read_entries(&ledger);
    assert_ne!(reads, Vec::<Entry>::new());
    for entry in reads {
        let file = entry.file.clone().expect("a read records its item");
        assert_eq!(file.path, entry.path);
        assert_eq!(acl.file_id(&entry.path).unwrap(), Some(file.id));
    }
}

#[test]
fn a_granted_folder_moved_during_the_run_loses_the_read_where_it_went() {
    let w = World::new("files-moved");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let moved = move_docs(&w, &mut acl, "other/m2/moved");
    assert!(allows_in_other(&w, &acl).iter().any(|k| k.0 == moved));
    acl.walks.clear();

    let ended = end(&w, &mut ledger, &mut acl);
    assert!(
        ended
            .removed
            .contains(&w.canonical("other/m2").join("docs"))
    );
    assert_eq!(allows_in_other(&w, &acl), Vec::<Key>::new());
    assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
    // Its tree is walked where it went, so the inherited copies there go too.
    assert!(acl.walks.iter().any(|(r, _, _)| *r == moved));
    assert!(acl.walks.iter().any(|(r, _, _)| *r == moved.join("deep")));
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the other matter's agent or the lawyer deletes the folder, not the sandbox"
)]
fn a_granted_folder_deleted_during_the_run_counts_as_taken_back() {
    let w = World::new("files-deleted");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let docs = w.canonical("other/m2/docs");
    std::fs::remove_dir_all(&docs).unwrap();
    acl.delete_item(&docs);
    acl.walks.clear();

    let ended = end(&w, &mut ledger, &mut acl);
    assert_eq!(ended.removed, [docs.clone(), docs.join("deep")]);
    assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
    assert_eq!(allows_in_other(&w, &acl), Vec::<Key>::new());
    // Nothing is left to walk.
    assert!(acl.walks.iter().all(|(r, _, _)| !r.starts_with(&docs)));
}

#[test]
fn a_new_item_at_a_granted_path_is_left_alone_and_the_old_one_loses_the_read() {
    let w = World::new("files-replaced");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let old = move_docs(&w, &mut acl, "other/m2/old");
    w.dir.file("other/m2/docs/new.txt", "n");
    let docs = w.canonical("other/m2/docs");
    acl.make_new(&docs);
    // What the new item carries (its own entries, the lawyer's say), of the same shape.
    let theirs = (
        docs.clone(),
        USER_SID.to_owned(),
        Rights::ReadAndExecute,
        Inherit::ThisOnly,
    );
    acl.aces.insert(theirs.clone());

    end(&w, &mut ledger, &mut acl);
    assert_eq!(allows_in_other(&w, &acl), vec![theirs]);
    assert!(acl.aces.iter().all(|k| !k.0.starts_with(&old)));
    assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
}

#[test]
fn a_relaunch_takes_back_a_read_whose_item_moved_and_writes_it_on_the_item_there_now() {
    let w = World::new("files-relaunch");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let moved = move_docs(&w, &mut acl, "other/m2/moved");
    w.dir.file("other/m2/docs/new.txt", "n");
    let docs = w.canonical("other/m2/docs");
    acl.make_new(&docs);

    let mut ledger = super::ledger(&w);
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let left: Vec<&Key> = acl
        .aces
        .iter()
        .filter(|k| k.0.starts_with(&moved))
        .collect();
    assert_eq!(left, Vec::<&Key>::new());
    let now = acl.file_id(&docs).unwrap();
    let reads = read_entries(&ledger);
    assert!(
        reads
            .iter()
            .all(|e| e.path == docs && e.file.as_ref().map(|f| f.id) == now)
    );
    assert_eq!(reads.len(), 2, "the folder alone and its files");
    let on_docs = allows_in_other(&w, &acl);
    assert!(on_docs.iter().all(|k| k.0 == docs) && on_docs.len() == 2);
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "writes the ledger as an older version left it"
)]
fn an_older_ledgers_read_with_no_file_loads_and_is_written_again_with_one() {
    let w = World::new("files-older");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    // The ledger as a version before R18's file IDs wrote it: no `file` on a read.
    let path = ledger_path(&w.at("data"));
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for entry in json["entries"].as_array_mut().unwrap() {
        if entry["kind"] == "other_matter_read" {
            entry.as_object_mut().unwrap().remove("file");
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    let mut ledger = super::ledger(&w);
    let reads = read_entries(&ledger);
    assert_ne!(reads, Vec::<Entry>::new());
    assert!(reads.iter().all(|e| e.file.is_none()));

    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let reads = read_entries(&ledger);
    assert_ne!(reads, Vec::<Entry>::new());
    assert!(reads.iter().all(|e| e.file.is_some()));
    end(&w, &mut ledger, &mut acl);
    assert_eq!(allows_in_other(&w, &acl), Vec::<Key>::new());
}

#[test]
fn start_takes_back_every_read_a_crashed_run_left_wherever_its_item_went() {
    let w = World::new("files-start");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    // The run crashes: no end. Its folder was moved meanwhile.
    launch(&w, &mut ledger, &mut acl, &[("other/m2/docs", false)]);
    let moved = move_docs(&w, &mut acl, "other/m2/moved");
    acl.walks.clear();

    let mut ledger = super::ledger(&w);
    let done = reconcile(&mut ledger, &mut acl).unwrap();
    assert_eq!(done.removed, 4, "two entries on each folder");
    assert_eq!(done.unchecked, 0);
    assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
    assert_eq!(allows_in_other(&w, &acl), Vec::<Key>::new());
    assert!(acl.walks.iter().any(|(r, _, _)| *r == moved));
    // The launch's other entries stay.
    assert!(ledger.entries().any(|e| e.kind == PathKind::Matter));
}

/// Whether the launch refused as `failure` naming `OtherMatterRead`, at `at` when given, and
/// left no read behind.
fn assert_read_refused(w: &World, failure: ApplyFailure, at: Option<&Path>, plan_fenced: bool) {
    let (mut ledger, mut acl) = (ledger(w), volume(w));
    if let Some(at) = at {
        let planted = (
            at.to_owned(),
            USER_SID.to_owned(),
            Rights::Modify,
            Inherit::ThisOnly,
        );
        acl.aces.insert(planted);
    } else {
        acl.fail_walk = Some((w.canonical("other/m2/docs/deep"), failure));
    }
    let mut plan = read_plan(w, &[("other/m2/docs", false)], plan_fenced);
    let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(w)).unwrap_err();
    let Error::AclApply {
        path_kind,
        failure: f,
        at: named,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!((*path_kind, *f), (PathKind::OtherMatterRead, failure));
    if let Some(at) = at {
        assert_eq!(named.as_deref(), Some(at));
    }
    assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
    let left = allows_in_other(w, &acl);
    assert!(left.iter().all(|k| Some(k.0.as_path()) == at), "{left:?}");
}

#[test]
fn a_read_into_a_folder_with_inheritance_off_or_a_beating_allow_refuses_the_launch() {
    // Matter two's fence is applied already, so no fence walk of its tree runs in this launch:
    // the read's own strict check refuses.
    let w = World::new("files-strict-protected");
    w.other_tree();
    assert_read_refused(&w, ApplyFailure::ProtectedChild, None, true);
    let w = World::new("files-strict-broad");
    w.other_tree();
    let b = w.canonical("other/m2/docs/b.txt");
    assert_read_refused(&w, ApplyFailure::BroadAllowChild, Some(&b), true);
}

#[test]
fn the_reads_check_walks_each_granted_folders_children_strictly_and_nothing_else() {
    let w = World::new("files-strict-walks");
    w.other_tree();
    let (mut ledger, mut acl) = (ledger(&w), volume(&w));
    let mut plan = read_plan(&w, &[("other/m2/docs", false)], true);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let checks: Vec<PathBuf> = acl
        .walks
        .iter()
        .filter(|(_, reach, strict)| *reach == Reach::Children && *strict)
        .map(|(r, ..)| r.clone())
        .collect();
    let docs = w.canonical("other/m2/docs");
    assert_eq!(checks, [docs.clone(), docs.join("deep")]);
}

#[test]
fn the_top_granted_folder_with_inheritance_off_or_a_beating_allow_refuses_the_launch() {
    for protected in [true, false] {
        let w = World::new("files-strict-top");
        w.other_tree();
        let docs = w.canonical("other/m2/docs");
        let (mut ledger, mut acl) = (ledger(&w), volume(&w));
        if protected {
            acl.protected.insert(docs.clone());
        } else {
            let planted = (
                docs.clone(),
                USER_SID.to_owned(),
                Rights::Modify,
                Inherit::ThisOnly,
            );
            acl.aces.insert(planted);
        }
        let mut plan = read_plan(&w, &[("other/m2/docs", false)], true);
        let error = apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap_err();
        let failure = if protected {
            ApplyFailure::ProtectedChild
        } else {
            ApplyFailure::BroadAllowChild
        };
        let Error::AclApply {
            path_kind,
            failure: f,
            at,
        } = &error
        else {
            panic!("{error:?}");
        };
        assert_eq!((*path_kind, *f), (PathKind::OtherMatterRead, failure));
        assert_eq!(at.as_deref(), Some(docs.as_path()));
        assert_eq!(read_entries(&ledger), Vec::<Entry>::new());
        // Nothing was written: only the planted allow is there.
        let left = allows_in_other(&w, &acl);
        assert!(
            left.iter().all(|k| k.0 == docs && k.2 == Rights::Modify),
            "{left:?}"
        );
    }
}

#[test]
fn the_top_of_each_read_is_checked_on_its_own_but_a_matters_root() {
    let w = World::new("files-strict-tops");
    w.other_tree();
    let (mut ledger, mut acl) = (super::ledger(&w), volume(&w));
    let reads = [("other/m2/docs", false), ("other/m2/a.txt", true)];
    let mut plan = read_plan(&w, &reads, true);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    let a = w.canonical("other/m2/a.txt");
    assert_eq!(acl.items, [a, w.canonical("other/m2/docs")]);

    // A read of the whole folder: its root carries the fence's explicit deny, and isn't
    // checked; each folder in it is checked as a child of its folder.
    let w = World::new("files-strict-root");
    w.other_tree();
    let (mut ledger, mut acl) = (super::ledger(&w), volume(&w));
    acl.broad = Broad::Ignored;
    let mut plan = read_plan(&w, &[("other/m2", false)], false);
    apply(&mut ledger, &mut plan, &mut acl, &mut around(&w)).unwrap();
    assert_eq!(acl.items, Vec::<PathBuf>::new());
    let m2 = w.canonical("other/m2");
    let checked = |r: &Path| {
        acl.walks
            .iter()
            .any(|(w, reach, s)| w == r && *reach == Reach::Children && *s)
    };
    assert!(checked(&m2) && checked(&m2.join("docs")) && checked(&m2.join("sub")));
}
