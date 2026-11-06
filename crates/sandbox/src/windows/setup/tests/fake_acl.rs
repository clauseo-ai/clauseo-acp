//! The permission writes faked in memory, for the set-up engine's tests: [`FakeAcl`].

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::fake::FakeMachine;
use crate::windows::acl_plan::{Grant, Inherit, Rights, Sid};
use crate::windows::ledger::RecordedFile;
use crate::windows::writer::{
    AclWriter, Added, ApplyFailure, FileId, Label, Reach, Strict, Walked,
};

pub(super) type Key = (PathBuf, String, Rights, Inherit);

/// The permission writes in memory: the entries on each path, and the walks. An add on
/// `fail_add` fails; a walk of `fail_walk` fails.
#[derive(Debug, Default)]
pub(super) struct FakeAcl {
    pub(super) aces: BTreeSet<Key>,
    /// Each add, and whether the thread acted as the owner then (`FakeMachine::acting`).
    pub(super) adds: Vec<(PathBuf, bool)>,
    /// The machine's `acting`, when made with [`FakeAcl::of`].
    pub(super) acting: Rc<Cell<bool>>,
    pub(super) walks: Vec<PathBuf>,
    pub(super) removed: Vec<Key>,
    pub(super) fail_add: Option<PathBuf>,
    pub(super) fail_walk: Option<PathBuf>,
    /// Each path's owner, when not the lawyer: a hand-back of one a pool user owns takes it off
    /// (the lawyer's again), unless it is in `keeps_owner`.
    pub(super) owners: BTreeMap<PathBuf, Sid>,
    /// Paths whose hand-back fails as `NtfsAcl`'s does on a handle with no `WRITE_OWNER`.
    pub(super) keeps_owner: BTreeSet<PathBuf>,
}

pub(super) fn key(grant: &Grant) -> Key {
    (
        grant.path().to_owned(),
        grant.sid().as_str().to_owned(),
        grant.rights(),
        grant.inherit(),
    )
}

impl FakeAcl {
    /// A writer that records, for each add, whether `machine` acted as the owner then.
    pub(super) fn of(machine: &FakeMachine) -> FakeAcl {
        FakeAcl {
            acting: Rc::clone(&machine.acting),
            ..FakeAcl::default()
        }
    }

    /// Whether an entry for `sid` with `rights` is on `path`.
    pub(super) fn has(&self, path: &Path, sid: &Sid, rights: Rights) -> bool {
        self.aces
            .iter()
            .any(|(p, s, r, _)| p == path && s == sid.as_str() && *r == rights)
    }
}

impl AclWriter for FakeAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        self.adds.push((grant.path().to_owned(), self.acting.get()));
        if self.fail_add.as_deref() == Some(grant.path()) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        Ok(if self.aces.insert(key(grant)) {
            Added::Written
        } else {
            Added::AlreadyThere
        })
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        self.aces.remove(&key(grant));
        self.removed.push(key(grant));
        Ok(())
    }

    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        Ok(self.aces.contains(&key(grant)))
    }

    fn propagate(
        &mut self,
        root: &Path,
        _reach: Reach,
        _strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.walks.push(root.to_owned());
        if self.fail_walk.as_deref() == Some(root) {
            return Err(ApplyFailure::ProtectedChild);
        }
        Ok(Walked::default())
    }

    fn same_file(&mut self, _a: &Path, _b: &Path) -> io::Result<bool> {
        Ok(false)
    }

    fn file_id(&mut self, _path: &Path) -> io::Result<Option<FileId>> {
        Ok(None)
    }

    fn remove_recorded(
        &mut self,
        _file: &RecordedFile,
        grant: &Grant,
        _prior: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        self.remove(grant)
    }

    fn present_recorded(&mut self, _file: &RecordedFile, grant: &Grant) -> io::Result<bool> {
        self.present(grant)
    }

    fn add_expecting(&mut self, grant: &Grant, _expected: &Path) -> Result<Added, ApplyFailure> {
        self.add(grant)
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        None
    }

    fn reclaim(&mut self, _root: &Path, _pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        Ok(Walked::default())
    }

    fn hand_back_tree(&mut self, _root: &Path, _pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        Ok(Walked::default())
    }

    fn hand_back(&mut self, path: &Path, pool: &[Sid]) -> Result<bool, ApplyFailure> {
        if !self.owners.get(path).is_some_and(|o| pool.contains(o)) {
            return Ok(false);
        }
        if self.keeps_owner.contains(path) {
            return Err(ApplyFailure::ChildShut);
        }
        self.owners.remove(path);
        Ok(true)
    }

    fn label(&mut self, _path: &Path) -> io::Result<Option<Label>> {
        Ok(None)
    }

    fn restore_label(&mut self, _grant: &Grant, _label: Label) -> Result<(), ApplyFailure> {
        Ok(())
    }
}
