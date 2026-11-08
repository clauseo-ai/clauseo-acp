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
