//! The gate's edge for the other matters: every path the launcher passes is resolved once, to
//! its canonical spelling, and each fence the plan will carry is checked like the launching
//! matter's own folders.

use std::io;
use std::path::Path;

use super::{FolderRefusal, FolderSupport, is_reparse_point, refusal};
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

/// The fence of each registered matter not fenced yet, from `given` (the launcher's spelling,
/// read before resolving, so a link there is seen): a folder that doesn't exist is left out
/// (nothing to protect yet; the host's matter creation fences it); one on a network share is
/// left out too, as a sibling is; any other folder must have no odd spelling, be no link and
/// answer `Ok`, or the launch is refused with [`Error::NeighbourUnfenceable`] naming that matter.
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
            if odd_spelling(asked) || odd_spelling(folder) {
                return Err(refuse(FolderRefusal::OddSpelling));
            }
            match is_reparse_point(asked) {
                Ok(true) => return Err(refuse(FolderRefusal::ReparsePoint)),
                Ok(false) => {}
                Err(error) => return Err(refuse(FolderRefusal::CheckFailed(error.kind()))),
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
