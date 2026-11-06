//! N10: a crash during a move, settled on the next load so every item in a pass's folder has a
//! finished manifest entry; and N11: a set-aside that can't happen refuses, naming the item.

use std::ffi::{OsStr, OsString};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use super::{after, before, exit, exit_as, launch, list, none, record, volume, world};
use crate::windows::set_aside::files::{FileId, Place};
use crate::windows::set_aside::run::{self, Refused, Volume};
use crate::windows::set_aside::scan::Listed;
use crate::windows::set_aside::{Reason, SetAsideFailure};
use crate::windows::test_world::matter_one;

/// Where a stand-in volume's move stops.
#[derive(Debug, Clone, Copy)]
enum Stop {
    /// The process dies after the intent is written and before the move.
    CrashBefore,
    /// The process dies after the move and before the entry is finished.
    CrashAfter,
    /// The move fails with this.
    Fail(SetAsideFailure),
    /// The process dies while the moved item is handed to the desktop user (S15 `PR4a2`).
    CrashAtOwn,
}

/// The tests' volume whose moves stop as [`Stop`] says.
struct Stopping<V> {
    inner: V,
    stop: Stop,
}

impl<V: Volume> Volume for Stopping<V> {
    type Folder = V::Folder;

    fn open_root(&self, root: &Path) -> io::Result<V::Folder> {
        self.inner.open_root(root)
    }

    fn open_folder(&self, folder: &V::Folder, name: &OsStr) -> io::Result<V::Folder> {
        self.inner.open_folder(folder, name)
    }

    fn folder_id(&self, folder: &V::Folder) -> io::Result<FileId> {
        self.inner.folder_id(folder)
    }

    fn list(&self, folder: &V::Folder) -> io::Result<Vec<Listed>> {
        self.inner.list(folder)
    }

    fn probe(&self, folder: &V::Folder, name: &OsStr) -> io::Result<Option<FileId>> {
        self.inner.probe(folder, name)
    }

    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>> {
        self.inner.file_id(path)
    }

    fn owner(&self, path: &Path) -> io::Result<String> {
        self.inner.owner(path)
    }

    fn own(&self, path: &Path) -> io::Result<()> {
        if matches!(self.stop, Stop::CrashAtOwn) {
            panic!("the process dies during the hand-back");
        }
        self.inner.own(path)
    }

    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
    ) -> Result<(), SetAsideFailure> {
        match self.stop {
            Stop::CrashBefore => panic!("the process dies before the move"),
            Stop::CrashAfter => {
                self.inner.move_into(from, sandbox, parts, name)?;
                panic!("the process dies after the move")
            }
            Stop::Fail(failure) => Err(failure),
            Stop::CrashAtOwn => self.inner.move_into(from, sandbox, parts, name),
        }
    }

    fn move_back(
        &self,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
        to: &Path,
    ) -> Result<(), SetAsideFailure> {
        self.inner.move_back(sandbox, parts, name, to)
    }
}

fn stopping(stop: Stop) -> Stopping<impl Volume> {
    Stopping {
        inner: volume(),
        stop,
    }
}

/// The after-session pass of session `s1` through a volume that crashes as `stop` says.
fn crash_at_exit(root: &Path, data: &Path, stop: Stop) {
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        exit_as(&stopping(stop), root, data, "s1", exit())
    }));
    assert!(crashed.is_err(), "the pass died");
}

/// N10: the intent is written and the process dies before the move: the item is still in
/// the matter, so the next load drops the intent and lists nothing.
#[test]
fn a_crash_between_the_intent_and_the_move_is_settled_on_the_next_load() {
    let (w, root, data) = world("aside-crash-before");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/.mcp.json", "{}");

    crash_at_exit(&root, &data, Stop::CrashBefore);
    let left = Place::new(&data, &matter_one()).read_manifest().unwrap();
    assert_eq!(left.intents.len(), 1, "the intent was written first");
    assert_eq!(left.entries, none());

    assert_eq!(list(&data), none());
    let settled = Place::new(&data, &matter_one()).read_manifest().unwrap();
    assert_eq!(settled.intents, none());
    assert!(root.join(".mcp.json").is_file(), "never moved");
    assert_eq!(record(&data).sessions.len(), 1, "the session stays open");
    assert_eq!(after(&root, &data, exit()).unwrap().len(), 1);
}

/// N10: the item is moved and the process dies before its entry is finished: the next load
/// finds it in the pass's folder and finishes the entry, so every item there is listed.
#[test]
fn a_crash_between_the_move_and_the_finish_is_settled_on_the_next_load() {
    let (w, root, data) = world("aside-crash-after");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/.claude/settings.json", "{}");

    crash_at_exit(&root, &data, Stop::CrashAfter);
    let left = Place::new(&data, &matter_one()).read_manifest().unwrap();
    assert_eq!(left.intents.len(), 1);
    assert_eq!(left.entries, none());
    assert!(!root.join(".claude").exists(), "moved");

    let listed = list(&data);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].from, root.join(".claude"));
    assert!(listed[0].to.join("settings.json").is_file());
    let pass = listed[0].to.parent().unwrap();
    let mut in_pass: Vec<_> = std::fs::read_dir(pass)
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    in_pass.sort();
    let listed_names: Vec<_> = listed.iter().map(|e| e.name.clone()).collect();
    assert_eq!(in_pass, listed_names, "every item has a finished entry");
    let settled = Place::new(&data, &matter_one()).read_manifest().unwrap();
    assert_eq!(settled.intents, none());
}

/// N11: an item on another drive than the data folder, or in a folder that gives the lawyer
/// no `FILE_DELETE_CHILD` (the `C:\m` layout), can't be moved: the pass refuses and names it.
#[test]
fn a_set_aside_that_cant_happen_refuses_naming_the_item() {
    let cant = [
        SetAsideFailure::OtherVolume,
        SetAsideFailure::Io(io::ErrorKind::PermissionDenied),
    ];
    for failure in cant {
        let (w, root, data) = world("aside-cant-move");
        before(&root, &data, launch()).unwrap();
        w.dir.file("matter/.claude/settings.json", "{}");

        let refused = exit_as(&stopping(Stop::Fail(failure)), &root, &data, "s1", exit());
        let at = root.join(".claude");
        let named = Refused {
            failure,
            at: Some(at.clone()),
        };
        assert_eq!(refused, Err(named), "{failure:?}");
        assert!(at.is_dir(), "{failure:?}");
        assert_eq!(record(&data).sessions.len(), 1, "the record stays open");
        let message = crate::Error::SetAside {
            failure,
            at: Some(at.clone()),
        }
        .to_string();
        assert!(!message.contains(".claude"), "{message}");

        let item = w.dir.file("matter/sub/locked.txt", "shut out");
        let item = dunce::canonicalize(item).unwrap();
        let walked = run::set_aside_path(
            &stopping(Stop::Fail(failure)),
            &data,
            &matter_one(),
            &item,
            Reason::LawyerCantOpen,
            exit(),
        );
        let named = Refused {
            failure,
            at: Some(item.clone()),
        };
        assert_eq!(walked, Err(named), "{failure:?}");
        assert_eq!(list(&data), none(), "no entry for a move that failed");
    }
}

/// S15 `PR4a2`: the hand-back to the desktop user comes after the move and its finished entry,
/// so a crash during it leaves the item set aside and listed, never outside both places.
#[test]
fn a_crash_during_the_hand_back_leaves_the_item_set_aside_and_listed() {
    let (w, root, data) = world("aside-crash-own");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/sub/CLAUDE.md", "# the agent's\n");

    crash_at_exit(&root, &data, Stop::CrashAtOwn);
    let left = Place::new(&data, &matter_one()).read_manifest().unwrap();
    assert_eq!(left.intents, none());
    assert_eq!(left.entries.len(), 1, "{left:?}");
    assert!(left.entries[0].to.is_file());
    assert!(!root.join("sub/CLAUDE.md").exists());
}

/// S15 `PR4a2`: each item moved is handed to the desktop user, and its entry keeps the owner it
/// had, for the lawyer to see who made it.
#[cfg(unix)]
#[test]
fn each_item_set_aside_is_handed_to_the_desktop_user() {
    let (w, root, data) = world("aside-owned");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/sub/AGENTS.md", "# the agent's\n");
    let volume = super::posix::Posix::new(true);

    let moved = exit_as(&volume, &root, &data, "s1", exit()).unwrap();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(*volume.owned.borrow(), [moved[0].to.clone()]);
    assert!(
        moved[0]
            .owner
            .as_deref()
            .is_some_and(|o| o.starts_with("uid "))
    );
    assert_eq!(list(&data), moved);
}
