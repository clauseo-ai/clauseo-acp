//! Fences between matters (design B): each registered matter's own folders deny every pool user
//! but the one assigned to it. Exposed as [`plan_fence`], so PR4's set-up (when it makes the
//! pool) and the host's matter creation can fence a matter before any launch; every launch's plan
//! also carries the fence of each registered matter the ledger doesn't list as applied yet
//! ([`RegisteredMatter::fenced`]), so the first tier-B launch fences them all.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::acl_plan::{Grant, Inherit, Rights, Sid};
use super::admit::PathKind;

/// A matter's own folders: its root, its state folder and its notes folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatterFolders {
    /// The matter folder.
    pub root: PathBuf,
    /// Its agent's state folder.
    pub state: PathBuf,
    /// Its notes folder.
    pub notes: Option<PathBuf>,
}

impl MatterFolders {
    /// Each folder with its kind.
    pub fn each(&self) -> Vec<(&Path, PathKind)> {
        let mut folders = vec![
            (self.root.as_path(), PathKind::Matter),
            (self.state.as_path(), PathKind::State),
        ];
        if let Some(notes) = &self.notes {
            folders.push((notes.as_path(), PathKind::Notes));
        }
        folders
    }
}

/// Another registered matter, as the launcher passes it: its id, folders and assigned pool
/// user, and whether its fence is applied already (the launcher asks
/// [`Ledger::fence_applied`](super::ledger::Ledger::fence_applied)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredMatter {
    /// The matter's id: its fence's ledger entries are its own.
    pub id: MatterId,
    /// Its folders.
    pub folders: MatterFolders,
    /// The pool user assigned to it.
    pub pool_user: Sid,
    /// Whether the ledger lists its whole fence as applied; if not, this launch plans it.
    pub fenced: bool,
}

/// What the launcher knows about the other matters, for the gate and the plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Neighbours {
    /// Every other registered matter.
    pub registered: Vec<RegisteredMatter>,
    /// Every path any registered matter's policy reads, runs from or writes (the launcher
    /// computes it from their policies): no sibling deny lands on, above or inside one.
    pub reach: Vec<PathBuf>,
}

/// The fence of the matter with `folders`, assigned to `assigned`: on each of its folders, an
/// inherited deny of reads and writes for every SID of `pool` but `assigned`. A Low-labelled
/// matter that inherits `C:\`'s `Users` write entries is closed to other pool agents this way
/// (spike B, rows 2 and 4).
pub fn plan_fence(folders: &MatterFolders, assigned: &Sid, pool: &[Sid]) -> Vec<Grant> {
    let mut fence = Vec::new();
    for other in pool.iter().filter(|sid| *sid != assigned) {
        for (folder, kind) in folders.each() {
            fence.push(Grant {
                path: folder.to_owned(),
                sid: other.clone(),
                rights: Rights::DenyReadWrite,
                inherit: Inherit::Tree,
                kind,
            });
        }
    }
    fence
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(rid: u32) -> Sid {
        Sid::parse(&format!("S-1-5-21-1-2-3-{rid}")).unwrap()
    }

    #[test]
    fn a_fence_denies_every_pool_user_but_the_assigned_one_on_each_folder() {
        let folders = MatterFolders {
            root: PathBuf::from("/m"),
            state: PathBuf::from("/d/s"),
            notes: Some(PathBuf::from("/d/n")),
        };
        let pool = [sid(1001), sid(1002), sid(1003)];
        let fence = plan_fence(&folders, &pool[1], &pool);
        assert_eq!(fence.len(), 6);
        assert!(fence.iter().all(|g| g.sid != pool[1]));
        assert!(fence.iter().all(|g| g.rights == Rights::DenyReadWrite));
        assert!(fence.iter().all(|g| g.inherit == Inherit::Tree));
        for other in [&pool[0], &pool[2]] {
            for (folder, kind) in folders.each() {
                let found = fence
                    .iter()
                    .any(|g| &g.sid == other && g.path == folder && g.kind == kind);
                assert!(found, "{other} on {folder:?}");
            }
        }
    }
}
