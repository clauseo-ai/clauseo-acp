//! The take-back over a fake admin folder: a staging folder a refresh left SYSTEM-only (killed
//! midway, or holding the set-aside `.old` copy) is taken back and deleted, and the lock-down
//! and the removal then succeed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;

/// Paths with `/`, which Windows reads as a separator too, so the fake runs on every OS.
const DIR: &str = "pf/Clauseo/Sandbox";
const EXE: &str = "pf/Clauseo/Sandbox/clauseo-sandbox-setup.exe";
const SIG: &str = "pf/Clauseo/Sandbox/clauseo-sandbox-setup.exe.sig";
const STAGED: &str = "pf/Clauseo/Sandbox/staging";
const GIT_TREES: &str = "pf/Clauseo/Sandbox/git";

/// A fake folder tree: each path's kind and whether its DACL shuts Administrators out.
struct Tree {
    entries: BTreeMap<PathBuf, (Found, bool)>,
    /// The token holds the take-back privileges (an elevated one does).
    privileged: bool,
    calls: Vec<String>,
    /// What each lock and take-back was asked to leave out.
    skips: Vec<Vec<String>>,
}

impl Tree {
    /// The admin folder with the copy and its signature, all locked as `--adopt` leaves them.
    fn new() -> Tree {
        let mut tree = Tree {
            entries: BTreeMap::new(),
            privileged: true,
            calls: Vec::new(),
            skips: Vec::new(),
        };
        tree.add(DIR, Found::Folder, false);
        tree.add(EXE, Found::File, false);
        tree.add(SIG, Found::File, false);
        tree
    }

    fn add(&mut self, path: &str, kind: Found, shut: bool) {
        self.entries.insert(PathBuf::from(path), (kind, shut));
    }

    /// `path` and everything below it.
    fn below(&self, path: &Path) -> Vec<PathBuf> {
        self.entries
            .keys()
            .filter(|at| at.starts_with(path))
            .cloned()
            .collect()
    }

    /// Whether anything at or below `path` is shut.
    fn shut_below(&self, path: &Path) -> bool {
        self.below(path).iter().any(|at| self.entries[at].1)
    }

    /// Whether a folder above `path` is shut, so it can't be read.
    fn hidden(&self, path: &Path) -> bool {
        path.ancestors()
            .skip(1)
            .any(|above| self.entries.get(above).is_some_and(|(_, shut)| *shut))
    }
}

/// `path` with `/` alone, as the constants write it.
fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Windows' `ERROR_ACCESS_DENIED`, as `io::Error` reads it there.
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "denied")
}

impl LockDownFs for Tree {
    fn found(&mut self, path: &Path) -> io::Result<Found> {
        if self.hidden(path) {
            return Err(refused());
        }
        Ok(self
            .entries
            .get(path)
            .map_or(Found::Missing, |(kind, _)| *kind))
    }

    fn entries(&mut self, dir: &Path) -> io::Result<Vec<String>> {
        Ok(self
            .entries
            .keys()
            .filter(|at| at.parent() == Some(dir))
            .map(|at| at.file_name().unwrap().to_string_lossy().into_owned())
            .collect())
    }

    fn remove(&mut self, path: &Path, _kind: Found) -> io::Result<()> {
        self.calls.push(format!("remove {}", shown(path)));
        if self.shut_below(path) {
            return Err(refused());
        }
        for at in self.below(path) {
            self.entries.remove(&at);
        }
        Ok(())
    }

    fn lock_tree(&mut self, dir: &Path, skip: &[String]) -> io::Result<()> {
        self.calls.push(format!("lock {}", shown(dir)));
        self.skips.push(skip.to_vec());
        let skipped: Vec<PathBuf> = skip.iter().map(|name| dir.join(name)).collect();
        let shut = self
            .below(dir)
            .iter()
            .filter(|at| !skipped.iter().any(|skipped| at.starts_with(skipped)))
            .any(|at| self.entries[at].1);
        if shut { Err(refused()) } else { Ok(()) }
    }

    fn take_back(&mut self, path: &Path, skip: &[String]) -> io::Result<()> {
        self.calls.push(format!("take back {}", shown(path)));
        self.skips.push(skip.to_vec());
        if !self.privileged {
            return Err(io::Error::other("SeRestorePrivilege: not held"));
        }
        let skipped: Vec<PathBuf> = skip.iter().map(|name| path.join(name)).collect();
        for at in self.below(path) {
            if !skipped.iter().any(|skipped| at.starts_with(skipped)) {
                self.entries.get_mut(&at).unwrap().1 = false;
            }
        }
        Ok(())
    }
}

#[test]
fn a_staging_folder_a_killed_refresh_left_system_only_is_taken_back_and_the_repair_succeeds() {
    // Killed between making staging and its end: the folder and its staged `.new`, both
    // SYSTEM-only under the old STAGING_SDDL.
    let mut tree = Tree::new();
    tree.add(STAGED, Found::Folder, true);
    tree.add(
        &format!("{STAGED}/clauseo-sandbox-setup.exe.new"),
        Found::File,
        true,
    );
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(tree.found(Path::new(STAGED)).unwrap(), Found::Missing);
    assert_eq!(
        tree.calls,
        [
            format!("remove {STAGED}"),
            format!("take back {STAGED}"),
            format!("remove {STAGED}"),
            format!("lock {DIR}"),
        ]
    );
    assert_eq!(
        tree.found(Path::new(EXE)).unwrap(),
        Found::File,
        "the copy stays"
    );
    assert_eq!(tree.found(Path::new(SIG)).unwrap(), Found::File);
}

#[test]
fn a_refresh_whose_old_image_couldnt_be_deleted_leaves_an_old_copy_the_repair_clears() {
    // A successful refresh: its running image, set aside as `.old`, couldn't be deleted and
    // is only scheduled for the restart; a repair runs before it.
    let mut tree = Tree::new();
    tree.add(STAGED, Found::Folder, true);
    let old = format!("{STAGED}/clauseo-sandbox-setup.exe.old");
    tree.add(&old, Found::File, true);
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(tree.found(Path::new(&old)).unwrap(), Found::Missing);
    assert_eq!(tree.found(Path::new(STAGED)).unwrap(), Found::Missing);
    // Under the new STAGING_SDDL nothing is shut, and it goes with no take-back.
    let mut tree = Tree::new();
    tree.add(STAGED, Found::Folder, false);
    tree.add(&old, Found::File, false);
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(tree.found(Path::new(STAGED)).unwrap(), Found::Missing);
    assert!(
        !tree.calls.iter().any(|call| call.starts_with("take back")),
        "{:?}",
        tree.calls
    );
}

#[test]
fn a_staging_folder_that_cant_be_taken_back_never_fails_the_lock_down() {
    let mut tree = Tree::new();
    tree.privileged = false;
    tree.add(STAGED, Found::Folder, true);
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(
        tree.found(Path::new(STAGED)).unwrap(),
        Found::Folder,
        "left as SYSTEM's"
    );
    assert_eq!(tree.calls.last(), Some(&format!("lock {DIR}")));
}

#[test]
fn any_other_shut_entry_is_taken_back_and_deleted_and_the_copy_locked() {
    let mut tree = Tree::new();
    let stray = format!("{DIR}/version.dll");
    tree.add(&stray, Found::File, true);
    // The copy itself shut out: the lock-down takes the folder back rather than fail.
    tree.entries.get_mut(Path::new(EXE)).unwrap().1 = true;
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(tree.found(Path::new(&stray)).unwrap(), Found::Missing);
    assert_eq!(tree.calls.last(), Some(&format!("take back {DIR}")));
    assert!(!tree.shut_below(Path::new(DIR)));
}

#[test]
fn the_removal_takes_back_a_staging_folder_it_cant_open() {
    let mut tree = Tree::new();
    tree.add(STAGED, Found::Folder, true);
    tree.add(
        &format!("{STAGED}/clauseo-sandbox-setup.exe.old"),
        Found::File,
        true,
    );
    remove_taking_back(&mut tree, Path::new(STAGED)).unwrap();
    assert_eq!(tree.found(Path::new(STAGED)).unwrap(), Found::Missing);
    // Missing is fine; one that can't be taken back is the removal's error.
    remove_taking_back(&mut tree, Path::new(STAGED)).unwrap();
    let mut tree = Tree::new();
    tree.privileged = false;
    tree.add(STAGED, Found::Folder, true);
    assert!(remove_taking_back(&mut tree, Path::new(STAGED)).is_err());
}

#[test]
fn the_git_trees_are_kept_and_left_out_of_the_lock_down() {
    let mut tree = Tree::new();
    tree.add(GIT_TREES, Found::Folder, false);
    tree.add(&format!("{GIT_TREES}/ClauseoSb01"), Found::Folder, false);
    tree.add(
        &format!("{GIT_TREES}/ClauseoSb01/2.47.1.1-0123456789ab"),
        Found::Folder,
        false,
    );
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(tree.found(Path::new(GIT_TREES)).unwrap(), Found::Folder);
    assert_eq!(tree.calls, [format!("lock {DIR}")], "nothing removed");
    assert_eq!(tree.skips, [vec!["git".to_owned()]]);
}

#[test]
fn the_git_trees_shut_to_administrators_neither_fail_nor_get_taken_back() {
    // A tree's DACL grants SYSTEM, Administrators and its pool user; one an administrator
    // can't open still stays out of the lock-down, and the folder's take-back skips it too.
    let mut tree = Tree::new();
    tree.add(GIT_TREES, Found::Folder, true);
    tree.entries.get_mut(Path::new(EXE)).unwrap().1 = true;
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(
        tree.calls,
        [format!("lock {DIR}"), format!("take back {DIR}")]
    );
    assert_eq!(tree.skips, [vec!["git".to_owned()], vec!["git".to_owned()]]);
    assert!(
        tree.entries[Path::new(GIT_TREES)].1,
        "the trees weren't taken back"
    );
    assert!(!tree.entries[Path::new(EXE)].1);
    // In any case.
    let mut tree = Tree::new();
    tree.add("pf/Clauseo/Sandbox/GIT", Found::Folder, false);
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(
        tree.found(Path::new("pf/Clauseo/Sandbox/GIT")).unwrap(),
        Found::Folder
    );
}

#[test]
fn a_stuck_staging_folder_is_left_out_beside_the_git_trees() {
    let mut tree = Tree::new();
    tree.privileged = false;
    tree.add(STAGED, Found::Folder, true);
    tree.add(GIT_TREES, Found::Folder, false);
    lock_down_with(&mut tree, Path::new(DIR), Path::new(EXE)).unwrap();
    assert_eq!(
        tree.skips.last().unwrap(),
        &["git".to_owned(), "staging".to_owned()]
    );
}
