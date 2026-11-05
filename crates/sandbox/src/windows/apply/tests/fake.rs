//! The permission writes and the surroundings, faked in memory for the apply tests.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::windows::FolderSupport;
use crate::windows::acl_plan::{Grant, Inherit, Rights, Sid};
use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::writer::{AclWriter, Added, ApplyFailure, Label, Reach, Strict, Walked};

pub(super) type Key = (PathBuf, String, Rights, Inherit);

pub(super) fn key(grant: &Grant) -> Key {
    (
        grant.path().to_owned(),
        grant.sid().as_str().to_owned(),
        grant.rights(),
        grant.inherit(),
    )
}

/// What runs before each fake write.
type OnAdd = Box<dyn FnMut(&Grant)>;

/// The permission writes, in memory: the entries present, the walks asked for, and failures
/// to give.
#[derive(Default)]
pub(super) struct FakeAcl {
    pub(super) aces: BTreeSet<Key>,
    pub(super) fail_add: Option<PathBuf>,
    /// `fail_add` fails once only.
    pub(super) fail_add_once: bool,
    pub(super) fail_walk: Option<(PathBuf, ApplyFailure)>,
    pub(super) walks: Vec<(PathBuf, Reach, bool)>,
    /// The lawyer's own explicit labels, by file.
    pub(super) labels: BTreeMap<PathBuf, Label>,
    /// The labels put back, in order.
    pub(super) restored: Vec<(PathBuf, Label)>,
    /// The child the last failed walk refused.
    pub(super) refused: Option<PathBuf>,
    /// The trees handed back, with the pool users named.
    pub(super) reclaimed: Vec<(PathBuf, Vec<Sid>)>,
    /// `fail_walk` fails once only.
    pub(super) fail_once: bool,
    /// A walk of this root panics, as a crash would stop it.
    pub(super) panic_walk: Option<PathBuf>,
    /// A removal panics.
    pub(super) panic_remove: bool,
    /// Runs before each write, as another process acting meanwhile.
    pub(super) on_add: Option<OnAdd>,
    /// A hand-back walk of this root fails, `PermissionDenied` on its `child`.
    pub(super) fail_reclaim: Option<PathBuf>,
    /// Children the lawyer can't open: a strict or hand-back walk over one refuses it, once
    /// for each time it is listed (a set-aside moves it).
    pub(super) shut: Vec<PathBuf>,
    /// Other names for one file: each key's entries and label live on its value's.
    pub(super) links: BTreeMap<PathBuf, PathBuf>,
}

impl FakeAcl {
    /// The file `path` names.
    fn file(&self, path: &Path) -> PathBuf {
        self.links
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned())
    }

    /// `grant`'s key on its file.
    pub(super) fn key_of(&self, grant: &Grant) -> Key {
        let (_, sid, rights, inherit) = key(grant);
        (self.file(grant.path()), sid, rights, inherit)
    }

    /// A shut child below `root`, taken off the list.
    fn shut_below(&mut self, root: &Path) -> Option<PathBuf> {
        let at = self
            .shut
            .iter()
            .position(|c| c.starts_with(root) && c != root)?;
        Some(self.shut.remove(at))
    }
}

/// What a launch needs from around the matter, faked: the real listing, every folder `Ok`,
/// the set-asides asked for, and the credentials file.
#[derive(Debug, Default)]
pub(super) struct Around {
    pub(super) set_aside: Vec<PathBuf>,
    pub(super) credentials: Option<PathBuf>,
    /// Every set-aside fails (another volume, no `FILE_DELETE_CHILD`).
    pub(super) fail_set_aside: bool,
}

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, _folder: &Path) -> io::Result<FolderSupport> {
        Ok(FolderSupport::Ok)
    }

    fn set_aside(&mut self, path: &Path) -> io::Result<()> {
        if self.fail_set_aside {
            return Err(io::Error::from(io::ErrorKind::CrossesDevices));
        }
        self.set_aside.push(path.to_owned());
        Ok(())
    }

    fn credentials(&self) -> Option<PathBuf> {
        self.credentials.clone()
    }
}

impl AclWriter for FakeAcl {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        if let Some(hook) = self.on_add.as_mut() {
            hook(grant);
        }
        if self.fail_add.as_deref() == Some(grant.path()) {
            if self.fail_add_once {
                self.fail_add = None;
            }
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        let key = self.key_of(grant);
        Ok(if self.aces.insert(key) {
            Added::Written
        } else {
            Added::AlreadyThere
        })
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        assert!(!self.panic_remove, "a crash during a removal");
        let key = self.key_of(grant);
        self.aces.remove(&key);
        Ok(())
    }

    fn present(&mut self, grant: &Grant) -> io::Result<bool> {
        Ok(self.aces.contains(&self.key_of(grant)))
    }

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), reach, strict.is_some()));
        assert!(
            self.panic_walk.as_deref() != Some(root),
            "a crash during a walk"
        );
        if let Some((at, failure)) = self.fail_walk.clone()
            && at == root
            && strict.is_some()
        {
            if self.fail_once {
                self.fail_walk = None;
            }
            self.refused = Some(root.join("child"));
            return Err(failure);
        }
        if strict.is_some()
            && let Some(child) = self.shut_below(root)
        {
            self.refused = Some(child);
            return Err(ApplyFailure::ChildShut);
        }
        Ok(Walked {
            visited: 1,
            ..Walked::default()
        })
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        self.refused.take()
    }

    fn reclaim(&mut self, root: &Path, pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), Reach::Tree, false));
        if self.fail_reclaim.as_deref() == Some(root) {
            self.refused = Some(root.join("child"));
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        if let Some(child) = self.shut_below(root) {
            self.refused = Some(child);
            return Err(ApplyFailure::ChildShut);
        }
        self.reclaimed.push((root.to_owned(), pool.to_vec()));
        Ok(Walked::default())
    }

    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool> {
        Ok(a.file_name() == b.file_name())
    }

    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure> {
        if grant.path().file_name() != expected.file_name() {
            return Err(ApplyFailure::Changed);
        }
        self.add(grant)
    }

    /// The label on `path`'s file: ours when one of our label entries is there, else the
    /// lawyer's own.
    fn label(&mut self, path: &Path) -> io::Result<Option<Label>> {
        let file = self.file(path);
        let ours = self
            .aces
            .iter()
            .find(|(p, _, r, _)| *p == file && *r == Rights::LowLabel)
            .map(|(_, _, _, inherit)| Label {
                level: 4096,
                flags: if *inherit == Inherit::Tree { 0x3 } else { 0 },
                mask: 0x1,
            });
        Ok(ours.or_else(|| self.labels.get(&file).copied()))
    }

    fn restore_label(&mut self, grant: &Grant, label: Label) -> Result<(), ApplyFailure> {
        self.restored.push((grant.path().to_owned(), label));
        self.labels.insert(self.file(grant.path()), label);
        Ok(())
    }
}
