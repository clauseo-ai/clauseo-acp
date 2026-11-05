//! Step 3 of the gate: the folders beside the matter that the plan denies the pool group
//! reading, found through the [`FolderList`] seam.

use std::io;
use std::path::{Path, PathBuf};

use super::{
    FolderList, FolderRefusal, FolderSupport, PathKind, is_reparse_point, refusal, refused,
};
use crate::windows::acl_plan::in_folder;
use crate::windows::fence::Neighbours;
use crate::{Error, Policy};

/// What the walk found: the siblings, and the parent when its files are denied too.
pub(super) struct Walked {
    pub(super) siblings: Vec<PathBuf>,
    pub(super) parent_files: Option<PathBuf>,
}

/// The siblings the plan denies the pool group reading, and the parent whose files it denies:
/// see the gate's step 3. `neighbours` is already resolved.
pub(super) fn siblings(
    policy: &Policy,
    data: &Path,
    neighbours: &Neighbours,
    check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
    folders: &dyn FolderList,
) -> Result<Walked, Error> {
    let matter = policy.matter();
    let Some(parent) = matter.parent().filter(|parent| parent.parent().is_some()) else {
        return Ok(Walked {
            siblings: Vec::new(),
            parent_files: None,
        });
    };
    // Paths a deny may not touch at all, nor anything above them: this policy's own.
    let mut kept: Vec<PathBuf> = vec![policy.state().to_owned(), data.to_owned()];
    kept.extend(policy.notes().map(Path::to_owned));
    kept.extend(policy.home().map(Path::to_owned));
    kept.extend(policy.credential_link().map(Path::to_owned));
    kept.extend(policy.claude_store().map(|store| store.dir().to_owned()));
    kept.extend(policy.runtime().iter().cloned());
    kept.extend(policy.read().iter().cloned());
    kept.extend(policy.allowed_folders().iter().cloned());
    // Paths a deny may not touch, nor be above, but whose holders are descended into: the other
    // matters and everything their policies reach.
    let mut protected: Vec<PathBuf> = neighbours
        .registered
        .iter()
        .map(|other| other.folders.root.clone())
        .collect();
    protected.extend(neighbours.reach.iter().cloned());
    let walk = Walk {
        matter,
        kept,
        protected,
    };
    let parent_files = walk.parent_files(parent, data, policy, check)?;
    let mut found = Vec::new();
    walk.folder(parent, check, folders, &mut found)?;
    Ok(Walked {
        siblings: found,
        parent_files,
    })
}

/// What the walk leaves alone.
struct Walk<'p> {
    matter: &'p Path,
    kept: Vec<PathBuf>,
    protected: Vec<PathBuf>,
}

impl Walk<'_> {
    /// Whether the files directly in `parent` get the group's deny (one entry on the parent,
    /// object-inherit, inherit-only, no propagation: files there, no folder): not when the
    /// parent is or sits inside a path left alone, holds the profile, the data folder or the
    /// sandbox records (a grant there is refused), or has one of the paths left alone as a file
    /// directly in it (its own allow would still win over an inherited deny, but it is left out
    /// to be sure). Checked like a sibling.
    fn parent_files(
        &self,
        parent: &Path,
        data: &Path,
        policy: &Policy,
        check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
    ) -> Result<Option<PathBuf>, Error> {
        let all = || self.kept.iter().chain(&self.protected);
        let inside = all().any(|k| in_folder(parent, k));
        let holds_records = policy
            .home()
            .into_iter()
            .chain([data])
            .any(|p| in_folder(p, parent));
        let file_in_it = all().any(|k| k.parent().is_some_and(|p| p == parent) && k.is_file());
        if inside || holds_records || file_in_it {
            return Ok(None);
        }
        match check(parent) {
            Ok(FolderSupport::Ok) => Ok(Some(parent.to_owned())),
            Ok(FolderSupport::Network) => Ok(None),
            answer => match refusal(answer) {
                Some(refusal) => Err(refused(PathKind::ParentFiles, refusal)),
                None => Ok(None),
            },
        }
    }

    /// Takes each folder in `dir`: denied, left out, or (holding a protected path) walked into.
    fn folder(
        &self,
        dir: &Path,
        check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
        folders: &dyn FolderList,
        found: &mut Vec<PathBuf>,
    ) -> Result<(), Error> {
        let failed =
            |error: io::Error| refused(PathKind::Sibling, FolderRefusal::CheckFailed(error.kind()));
        for listed in folders.subfolders(dir).map_err(failed)? {
            let sibling = match dunce::canonicalize(&listed) {
                Ok(sibling) => sibling,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(failed(error)),
            };
            // A listing gives what is in `dir`; anything else is left alone, so the walk only
            // goes down.
            if !in_folder(&sibling, dir) || in_folder(dir, &sibling) {
                continue;
            }
            if is_reparse_point(&listed).map_err(failed)? {
                continue;
            }
            let overlaps = |a: &Path, b: &Path| in_folder(a, b) || in_folder(b, a);
            if overlaps(&sibling, self.matter) || self.kept.iter().any(|k| overlaps(&sibling, k)) {
                continue;
            }
            if self.protected.iter().any(|p| in_folder(&sibling, p)) {
                continue;
            }
            if self.protected.iter().any(|p| in_folder(p, &sibling)) {
                self.folder(&sibling, check, folders, found)?;
                continue;
            }
            match check(&sibling) {
                Ok(FolderSupport::Ok) => found.push(sibling),
                Ok(FolderSupport::Network) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                answer => {
                    if let Some(refusal) = refusal(answer) {
                        return Err(refused(PathKind::Sibling, refusal));
                    }
                }
            }
        }
        Ok(())
    }
}
