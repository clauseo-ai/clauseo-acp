//! The set-aside's bookkeeping deleted after real passes: a matter whose items were all set aside
//! and then restored leaves no pass folder, so `--remove`'s call and the uninstaller's both take
//! its set-aside folder whole; and a mark that fails partway on NTFS takes none. On Windows the
//! passes and the deletion are NTFS's, elsewhere the stand-ins'.

use std::ffi::OsStr;
use std::path::PathBuf;

use super::super::bookkeeping::{SetAsideBookkeeping, forget_bookkeeping};
use super::super::forget_set_aside_bookkeeping;
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
