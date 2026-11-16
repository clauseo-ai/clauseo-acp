//! The gate's edge for the other matters: every path the launcher passes is resolved once, to
//! its canonical spelling, and each fence the plan will carry is checked like the launching
//! matter's own folders.

use std::io;
use std::path::Path;

use super::{FolderRefusal, FolderSupport, refusal};
use crate::Error;
use crate::windows::acl_plan::targets::odd_spelling;
use crate::windows::fence::{Fence, Neighbours};

/// `neighbours` with every path canonical; a path that doesn't resolve keeps its spelling.
pub(super) fn resolve(neighbours: &Neighbours) -> Neighbours {
    let canonical = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_missing| p.to_owned());
    let mut resolved = neighbours.clone();
    for other in &mut resolved.registered {
        other.folders.root = canonical(&other.folders.root);
        other.folders.state = canonical(&other.folders.state);
        other.folders.notes = other.folders.notes.as_deref().map(canonical);
    }
    resolved.reach = neighbours.reach.iter().map(|p| canonical(p)).collect();
    resolved
}

/// The fence of each registered matter not fenced yet, each folder checked exactly as that
/// matter's own launch checks it, so a neighbour is refused only where its own launch would be
/// too. Its own policy refuses a folder whose own name, as given, is a symbolic link or junction
/// (a name-surrogate reparse point; `policy/build.rs`'s `root_folder`), and resolves every link
/// above it; its own plan refuses an odd spelling of the resolved path. So here: a folder that
/// doesn't exist is left out (nothing to protect yet; the host's matter creation fences it); one
/// on a network share is left out too, as a sibling is; a folder reached through a linked parent
/// is fenced at its resolved target; a cloud placeholder or other reparse point that isn't a
/// name surrogate is accepted, as D7 does. A folder that is itself a link, has an odd resolved
/// spelling, or can't take the fence refuses the launch with [`Error::NeighbourUnfenceable`]
/// naming that matter.
pub(super) fn fences(
    resolved: &Neighbours,
    given: &Neighbours,
    check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
) -> Result<Vec<Fence>, Error> {
    let mut fences = Vec::new();
    for (other, spelled) in resolved.registered.iter().zip(&given.registered) {
        if other.fenced {
            continue;
        }
        let mut folders = Vec::new();
        for ((folder, kind), (asked, _)) in
            other.folders.each().into_iter().zip(spelled.folders.each())
        {
            let refuse = |support| Error::NeighbourUnfenceable {
                matter: other.id,
                path_kind: kind,
                support,
            };
            match own_name_is_link(asked) {
                Ok(true) => return Err(refuse(FolderRefusal::ReparsePoint)),
                Ok(false) => {}
                Err(error) => return Err(refuse(FolderRefusal::CheckFailed(error.kind()))),
            }
            if odd_spelling(folder) {
                return Err(refuse(FolderRefusal::OddSpelling));
            }
            match check(folder) {
                Ok(FolderSupport::Ok) => folders.push((folder.to_owned(), kind)),
                Ok(FolderSupport::Network) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                answer => {
                    if let Some(refusal) = refusal(answer) {
                        tracing::info!(%kind, %refusal, "tier B can't fence another matter");
                        return Err(refuse(refusal));
                    }
                }
            }
        }
        if !folders.is_empty() {
            fences.push(Fence {
                id: other.id,
                pool_user: other.pool_user.clone(),
                folders,
            });
        }
    }
    Ok(fences)
}

/// Whether the name `path` itself is a symbolic link or junction, as the policy's own check
/// reads it (`symlink_metadata`'s `is_symlink`: name-surrogate reparse points only); a missing
/// path is not.
fn own_name_is_link(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(meta.file_type().is_symlink()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
