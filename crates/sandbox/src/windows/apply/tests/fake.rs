//! The permission writes and the surroundings, faked in memory for the apply tests.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use crate::windows::FolderSupport;
use crate::windows::acl_plan::{Grant, Inherit, Rights, Sid};
use crate::windows::admit::{FolderList, PathKind, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::ledger::RecordedFile;
use crate::windows::writer::{
    AclWriter, Added, ApplyFailure, FileId, Label, Reach, Strict, Walked,
};

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
    /// A check of whether an entry on this path is there panics.
    pub(super) panic_present: Option<PathBuf>,
    /// Runs before each write, as another process acting meanwhile.
    pub(super) on_add: Option<OnAdd>,
    /// A hand-back walk of this root fails, `PermissionDenied` on its `child`.
    pub(super) fail_reclaim: Option<PathBuf>,
    /// Children the lawyer can't open: a strict or hand-back walk over one refuses it, once
    /// for each time it is listed (a set-aside moves it).
    pub(super) shut: Vec<PathBuf>,
    /// Put before a refused child's path when the walk reports it: `\\?\` for its verbatim
    /// final path, as the NTFS walk reports it; empty for its plain path.
    pub(super) prefix: &'static str,
    /// Other names for one file: each key's entries and label live on its value's.
    pub(super) links: BTreeMap<PathBuf, PathBuf>,
    /// The file each ID [`AclWriter::file_id`] gave names: what an open by ID finds.
    pub(super) issued: BTreeMap<(u64, [u8; 16]), PathBuf>,
    /// Paths with no file at all.
    pub(super) gone: BTreeSet<PathBuf>,
    /// Files no longer there under any name: an open by their ID finds nothing.
    pub(super) vanished: BTreeSet<PathBuf>,
    /// A write on this path fails with this failure, as `NtfsAcl` would (S15 `PR4a2`).
    pub(super) fail_add_with: Option<(PathBuf, ApplyFailure)>,
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

    /// The file `file` recorded, found by its ID wherever its names are now; `None` when it
    /// is no longer there.
    fn by_id(&self, file: &RecordedFile) -> Option<PathBuf> {
        let found = self.issued.get(&(file.id.volume, file.id.id))?;
        (!self.vanished.contains(found)).then(|| found.clone())
    }

    /// A shut child below `root`, taken off the list, as the walk reports it.
    fn shut_below(&mut self, root: &Path) -> Option<PathBuf> {
        let at = self
            .shut
            .iter()
            .position(|c| c.starts_with(root) && c != root)?;
        let child = self.shut.remove(at);
        Some(PathBuf::from(format!("{}{}", self.prefix, child.display())))
    }
}

/// What a launch needs from around the matter, faked: the real listing, every folder `Ok`,
/// the set-asides asked for, and the credentials file.
#[derive(Debug, Default)]
pub(super) struct Around {
    pub(super) set_aside: Vec<PathBuf>,
    /// The matter each of `set_aside` went to, in the same order.
    pub(super) owners: Vec<MatterId>,
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

    fn set_aside(&mut self, matter: MatterId, path: &Path) -> io::Result<()> {
        if self.fail_set_aside {
            return Err(io::Error::from(io::ErrorKind::CrossesDevices));
        }
        self.set_aside.push(path.to_owned());
        self.owners.push(matter);
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
        if let Some((_, failure)) = self
            .fail_add_with
            .as_ref()
            .filter(|(p, _)| p == grant.path())
        {
            return Err(*failure);
        }
        // `NtfsAcl`'s rule: a protected name in the matter that is a link on disk.
        let link =
            std::fs::symlink_metadata(grant.path()).is_ok_and(|m| m.file_type().is_symlink());
        if link && grant.kind() == PathKind::ReadOnlyInMatter {
            return Err(ApplyFailure::Link);
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
        assert!(
            self.panic_present.as_deref() != Some(grant.path()),
            "a crash during a check"
        );
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

    /// Two paths with one file name are one file. From then on `b` (and its canonical form),
    /// unless `links` names it already, is a name for the file `a` names now: the credentials
    /// file, once checked against the credential link, stays the file written through the link
    /// when the link is swapped later.
    fn same_file(&mut self, a: &Path, b: &Path) -> io::Result<bool> {
        if a.file_name() != b.file_name() {
            return Ok(false);
        }
        let file = self.file(a);
        let canonical = b
            .parent()
            .and_then(|folder| dunce::canonicalize(folder).ok())
            .zip(b.file_name())
            .map(|(folder, name)| folder.join(name));
        for name in std::iter::once(b.to_owned()).chain(canonical) {
            if name != file {
                self.links.entry(name).or_insert_with(|| file.clone());
            }
        }
        Ok(true)
    }

    fn add_expecting(&mut self, grant: &Grant, expected: &Path) -> Result<Added, ApplyFailure> {
        if !self.same_file(grant.path(), expected).unwrap_or(false) {
            return Err(ApplyFailure::Changed);
        }
        self.add(grant)
    }

    /// A made-up ID from the file `path` names, kept in `issued`.
    fn file_id(&mut self, path: &Path) -> io::Result<Option<FileId>> {
        if self.gone.contains(path) {
            return Ok(None);
        }
        let file = self.file(path);
        let half = |salt: u8| {
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&(salt, &file), &mut hasher);
            std::hash::Hasher::finish(&hasher).to_le_bytes()
        };
        let mut id = [0; 16];
        let (low, high) = id.split_at_mut(8);
        low.copy_from_slice(&half(0));
        high.copy_from_slice(&half(1));
        let id = FileId { volume: 1, id };
        self.issued.insert((id.volume, id.id), file);
        Ok(Some(id))
    }

    /// The entry comes off the file `file` recorded, keyed by that file whatever `grant`'s path
    /// names now, and `prior` goes back on it.
    fn remove_recorded(
        &mut self,
        file: &RecordedFile,
        grant: &Grant,
        prior: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        assert!(!self.panic_remove, "a crash during a removal");
        let Some(found) = self.by_id(file) else {
            return Ok(());
        };
        let (_, sid, rights, inherit) = key(grant);
        self.aces.remove(&(found.clone(), sid, rights, inherit));
        if let Some(label) = prior {
            self.restored.push((found.clone(), label));
            self.labels.insert(found, label);
        }
        Ok(())
    }

    fn present_recorded(&mut self, file: &RecordedFile, grant: &Grant) -> io::Result<bool> {
        let Some(found) = self.by_id(file) else {
            return Ok(false);
        };
        let (_, sid, rights, inherit) = key(grant);
        Ok(self.aces.contains(&(found, sid, rights, inherit)))
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
