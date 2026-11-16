//! The set-aside's bookkeeping deleted after real passes: a matter whose items were all set aside
//! and then restored leaves no pass folder, so `--remove`'s call and the uninstaller's both take
//! its set-aside folder whole; and a mark that fails partway on NTFS takes none. On Windows the
//! passes and the deletion are NTFS's, elsewhere the stand-ins'.

use std::ffi::OsStr;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use super::super::Entry;
use super::super::SetAsideFailure;
use super::super::bookkeeping::{
    SetAsideBookkeeping, forget_bookkeeping, forget_bookkeeping_within,
};
use super::super::files::Place;
use super::super::forget_set_aside_bookkeeping;
use super::super::run::kept_levels;
use super::{after, before, exit, launch, list, run, volume, world};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::set_aside::SET_ASIDE_DIR;
use crate::windows::test_world::matter_one;

/// `<data>\sandbox\set-aside\<matter one>`.
fn matter_folder(data: &std::path::Path) -> PathBuf {
    data.join(SANDBOX_DIR)
        .join(SET_ASIDE_DIR)
        .join(matter_one().to_string())
}

/// The names in `folder`, sorted.
fn names(folder: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_matter_whose_items_were_all_restored_goes_whole_for_remove_and_the_uninstaller() {
    for uninstaller in [false, true] {
        let (w, root, data) = world(&format!("aside-restored-{uninstaller}"));
        before(&root, &data, launch()).unwrap();
        // One item at the root, one nested (a `within` folder in its pass folder).
        w.dir.file("matter/CLAUDE.md", "# the agent's\n");
        w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
        let entries = after(&root, &data, exit()).unwrap();
        assert_eq!(entries.len(), 2, "{entries:?}");
        let folder = matter_folder(&data);
        assert!(names(&folder).iter().any(|name| name.ends_with('Z')));
        for entry in &entries {
            run::restore(&volume(), &data, entry).unwrap();
        }
        assert_eq!(list(&data), Vec::new());
        // The emptied pass folder went with the last restore: only bookkeeping is left.
        assert_eq!(
            names(&folder),
            ["manifest.json", "record.json", "set-aside.lock"]
        );
        let gone = if uninstaller {
            let set_aside = folder.parent().unwrap();
            forget_set_aside_bookkeeping(set_aside, OsStr::new(&matter_one().to_string()))
        } else {
            forget_bookkeeping(&data, &matter_one())
        };
        assert_eq!(gone, Ok(SetAsideBookkeeping::Gone));
        assert!(std::fs::symlink_metadata(&folder).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
            "# the agent's\n"
        );
        assert!(root.join("sub").join(".git").join("HEAD").exists());
    }
}

#[test]
fn a_restore_leaves_a_pass_folder_still_holding_another_item() {
    let (w, root, data) = world("aside-restored-one-of-two");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/CLAUDE.md", "# the agent's\n");
    w.dir.file("matter/AGENTS.md", "# the agent's\n");
    let mut entries = after(&root, &data, exit()).unwrap();
    assert_eq!(entries.len(), 2, "{entries:?}");
    let kept = entries.pop().unwrap();
    run::restore(&volume(), &data, &entries[0]).unwrap();
    assert!(kept.to.is_file(), "{}", kept.to.display());
    assert_eq!(
        forget_bookkeeping(&data, &matter_one()),
        Ok(SetAsideBookkeeping::Left)
    );
    assert!(kept.to.is_file());
}

#[test]
fn an_empty_folder_item_on_a_siblings_way_survives_the_siblings_restore() {
    // A move reuses a folder already there, so an item that is an empty folder can sit at the
    // path of a sibling's `within` folder: `<pass>\sub` an item, `<pass>\sub\.git` another.
    let (w, root, data) = world("aside-restored-folder-item");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    let git = after(&root, &data, exit()).unwrap().remove(0);
    assert_eq!(git.within, [std::ffi::OsString::from("sub")]);
    let place = Place::new(&data, &matter_one());
    let mut manifest = place.read_manifest().unwrap();
    let sub = Entry {
        id: manifest.next,
        from: root.join("other"),
        to: git.to.parent().unwrap().to_owned(),
        within: Vec::new(),
        name: "sub".into(),
        ..git.clone()
    };
    manifest.next += 1;
    manifest.entries.push(sub.clone());
    place.write_manifest(&manifest).unwrap();

    run::restore(&volume(), &data, &git).unwrap();
    assert!(sub.to.is_dir(), "{}", sub.to.display());
    assert_eq!(list(&data), [sub]);
    assert_eq!(
        forget_bookkeeping(&data, &matter_one()),
        Ok(SetAsideBookkeeping::Left)
    );
}

#[test]
fn forget_refuses_a_lock_still_held_and_leaves_the_record_as_it_was() {
    let (_w, root, data) = world("aside-forget-held");
    before(&root, &data, launch()).unwrap();
    after(&root, &data, exit()).unwrap();
    let folder = matter_folder(&data);
    let record = std::fs::read(folder.join("record.json")).unwrap();
    // Another holder of the matter's lock, as a writer is while it works.
    let held = crate::windows::write::lock_exclusive(&folder.join("set-aside.lock")).unwrap();
    assert_eq!(
        forget_bookkeeping_within(&data, &matter_one(), Duration::from_millis(100)),
        Err(SetAsideFailure::Io(io::ErrorKind::WouldBlock))
    );
    assert_eq!(names(&folder), ["record.json", "set-aside.lock"]);
    assert_eq!(std::fs::read(folder.join("record.json")).unwrap(), record);
    drop(held);
    assert_eq!(
        forget_bookkeeping(&data, &matter_one()),
        Ok(SetAsideBookkeeping::Gone)
    );
}

#[test]
fn the_kept_levels_stop_below_the_deepest_listed_folder() {
    let parts: Vec<std::ffi::OsString> = ["set-aside", "m", "pass", "a", "b"]
        .into_iter()
        .map(Into::into)
        .collect();
    let listed = |item: &[&str]| vec![item.iter().map(Into::into).collect::<Vec<_>>()];
    assert_eq!(kept_levels(&parts, &[], 2), 2);
    assert_eq!(
        kept_levels(&parts, &listed(&["set-aside", "m", "pass", "A"]), 2),
        4
    );
    assert_eq!(
        kept_levels(&parts, &listed(&["set-aside", "m", "pass", "a", "b"]), 2),
        5
    );
    assert_eq!(
        kept_levels(&parts, &listed(&["set-aside", "m", "other"]), 2),
        2
    );
}

#[cfg(windows)]
#[test]
fn a_mark_that_fails_partway_unmarks_the_ones_before_it() {
    // NTFS refuses to mark a read-only file for delete; the record, marked first (names list in
    // order), is unmarked, so it stays.
    let (_w, root, data) = world("aside-mark-fails");
    before(&root, &data, launch()).unwrap();
    after(&root, &data, exit()).unwrap();
    let folder = matter_folder(&data);
    assert_eq!(names(&folder), ["record.json", "set-aside.lock"]);
    let lock = folder.join("set-aside.lock");
    crate::test_support::set_read_only(&lock);
    assert!(forget_bookkeeping(&data, &matter_one()).is_err());
    assert_eq!(names(&folder), ["record.json", "set-aside.lock"]);
    // The scratch folder's removal takes the read-only file too (std ignores the attribute).
}
