//! The folder gate (S15's D1): before tier B plans or writes anything for a launch, every folder
//! it would grant on must take permission entries, and so must every other matter's folder that
//! a group in every matter's token can reach (D8). One refusal refuses the whole launch, and a
//! refusal has planned and written nothing: [`Admitted`] is the only way into
//! [`acl_plan::plan`](super::acl_plan::plan), and only the gate makes one.
//!
//! The gate's logic takes the folder check and the DACL reader as parameters ([`admit_with`]),
//! so its tests run on every OS; [`admit`] is the Windows call with the real folder check.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::FolderSupport;
use crate::{Error, Policy};

/// Which kind of path a grant or a refusal is about. Errors and logs name this, never the path,
/// so no matter's or client's name reaches them. Stored in the ledger by its snake-case name;
/// new kinds are appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// The matter folder.
    Matter,
    /// The agent's state folder.
    State,
    /// The matter's notes folder.
    Notes,
    /// The credential link inside the state folder.
    CredentialLink,
    /// Clauseo's Claude login home, the folder (`claude_store`).
    ClaudeStore,
    /// One of the allowed names in that folder ([`crate::CLAUDE_STORE_FILES`]).
    ClaudeStoreFile,
    /// A runtime path: Node, the adapter, the agent, the sidecars.
    Runtime,
    /// A read path: CA files, skills folders.
    Read,
    /// A folder the lawyer allowed, read-only.
    AllowedFolder,
    /// A write-denied path in the matter ([`Policy::read_only_in_matter`]).
    ReadOnlyInMatter,
    /// A write-denied path in the state folder ([`Policy::read_only_in_state`]).
    ReadOnlyInState,
    /// Another matter's folder that a group in every matter's token can reach (D8).
    OtherMatter,
}

impl fmt::Display for PathKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathKind::Matter => "the matter folder",
            PathKind::State => "the state folder",
            PathKind::Notes => "the notes folder",
            PathKind::CredentialLink => "the credential link",
            PathKind::ClaudeStore => "the Claude store folder",
            PathKind::ClaudeStoreFile => "a Claude store file",
            PathKind::Runtime => "a runtime path",
            PathKind::Read => "a read path",
            PathKind::AllowedFolder => "an allowed folder",
            PathKind::ReadOnlyInMatter => "a read-only path in the matter",
            PathKind::ReadOnlyInState => "a read-only path in the state folder",
            PathKind::OtherMatter => "another matter's folder",
        })
    }
}

/// Why the gate refused a folder: the folder check's answer (never [`FolderSupport::Ok`]), or
/// the kind of error the check failed with. Only the kind: the error's message may hold the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRefusal {
    /// The folder check answered, and not `Ok`.
    Support(FolderSupport),
    /// The folder check failed, as the host's `Answer::Failed` does: a missing folder, a file,
    /// a final path that can't be read.
    CheckFailed(io::ErrorKind),
}

impl fmt::Display for FolderRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolderRefusal::Support(FolderSupport::Ok) => f.write_str("it is supported"),
            FolderRefusal::Support(FolderSupport::CantChangePermissions) => {
                f.write_str("its permissions can't be changed")
            }
            FolderRefusal::Support(FolderSupport::Network) => {
                f.write_str("it is on a network drive")
            }
            FolderRefusal::Support(FolderSupport::NoPermissions) => {
                f.write_str("its drive keeps no permissions")
            }
            FolderRefusal::CheckFailed(kind) => write!(f, "the folder check failed ({kind})"),
        }
    }
}

/// Reads another matter's folder's DACL, for D8. A trait so tests can fake it; the Windows
/// reader comes with S15's ACL code (PR3).
pub trait GroupAccess {
    /// Whether `folder`'s DACL lets a group that every matter's restricted token holds reach it:
    /// any allow entry, inheritable ones included, that gives `BUILTIN\Users`, `Everyone`,
    /// `Authenticated Users` or `RESTRICTED` a read or write right. A missing folder is
    /// `NotFound`.
    fn group_can_reach(&self, folder: &Path) -> io::Result<bool>;
}

/// A launch the gate let through: every folder it grants on answered `Ok`. Holds the policy and
/// the other matters' folders this matter's SID must be denied (D8). Only the gate makes one.
#[derive(Debug)]
pub struct Admitted<'a> {
    policy: &'a Policy,
    closed: Vec<PathBuf>,
}

impl<'a> Admitted<'a> {
    /// The policy the gate checked.
    pub fn policy(&self) -> &'a Policy {
        self.policy
    }

    /// Other matters' folders a group in every token can reach, each answering `Ok`: the plan
    /// denies this matter's SID read and write on each.
    pub fn closed(&self) -> &[PathBuf] {
        &self.closed
    }
}

/// The folders tier B grants a writable right on, with their kinds: the matter, the state
/// folder, the notes folder and the Claude store folder. The credential link is a file inside
/// the state folder (the policy's rule), so the state folder's answer covers its volume.
pub fn grant_folders(policy: &Policy) -> Vec<(PathKind, &Path)> {
    let mut folders = vec![
        (PathKind::Matter, policy.matter()),
        (PathKind::State, policy.state()),
    ];
    if let Some(notes) = policy.notes() {
        folders.push((PathKind::Notes, notes));
    }
    if let Some(store) = policy.claude_store() {
        folders.push((PathKind::ClaudeStore, store.dir()));
    }
    folders
}

/// The gate on Windows: [`admit_with`] with [`folder_support`](super::folder_support).
#[cfg(windows)]
pub fn admit<'a>(
    policy: &'a Policy,
    others: &[PathBuf],
    groups: &dyn GroupAccess,
) -> Result<Admitted<'a>, Error> {
    admit_with(policy, others, super::folder_support, groups)
}

/// The gate, with the folder check `check` and the DACL reader `groups` as parameters.
///
/// 1. Each of [`grant_folders`] must answer [`FolderSupport::Ok`]; any other answer, or an
///    error, refuses with [`Error::FolderUnsupported`] naming the folder's kind.
/// 2. Each of `others`, the other matters' folders (their matter, state and notes folders):
///    when `groups` says a group can reach it, or can't tell, it is closed to this matter, and
///    so must answer `Ok` too. A `Network` folder is left alone: `ClauseoSandbox` is a local
///    account a file server doesn't know, and the restricted token was refused every operation
///    over `\\localhost` (`smb-spike.md`). A missing folder is left alone: nothing is there.
///    Any other answer refuses with [`PathKind::OtherMatter`].
///
/// Nothing is written. A refusal returns before anything is planned.
pub fn admit_with<'a>(
    policy: &'a Policy,
    others: &[PathBuf],
    mut check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    groups: &dyn GroupAccess,
) -> Result<Admitted<'a>, Error> {
    for (kind, folder) in grant_folders(policy) {
        if let Some(refusal) = refusal(check(folder)) {
            return Err(refused(kind, refusal));
        }
    }
    let mut closed = Vec::new();
    for other in others {
        match groups.group_can_reach(other) {
            Ok(false) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            // A DACL that can't be read is taken as open: the folder is closed if it can be.
            Ok(true) | Err(_) => {}
        }
        match check(other) {
            Ok(FolderSupport::Ok) => closed.push(other.clone()),
            Ok(FolderSupport::Network) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            answer => {
                if let Some(refusal) = refusal(answer) {
                    return Err(refused(PathKind::OtherMatter, refusal));
                }
            }
        }
    }
    Ok(Admitted { policy, closed })
}

/// `None` for `Ok`, else why the folder is refused.
fn refusal(answer: io::Result<FolderSupport>) -> Option<FolderRefusal> {
    match answer {
        Ok(FolderSupport::Ok) => None,
        Ok(support) => Some(FolderRefusal::Support(support)),
        Err(error) => Some(FolderRefusal::CheckFailed(error.kind())),
    }
}

/// The refusal, logged by kind only.
fn refused(path_kind: PathKind, support: FolderRefusal) -> Error {
    tracing::info!(%path_kind, %support, "tier B refused a folder; nothing was planned");
    Error::FolderUnsupported { path_kind, support }
}

#[cfg(test)]
mod tests;
