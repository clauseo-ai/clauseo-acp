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
// The gate itself builds on Windows and for tests; elsewhere only its types do.
#[cfg(any(windows, test, feature = "test-support"))]
use super::acl_plan::{AGENTS_DIR, in_folder};
#[cfg(any(windows, test, feature = "test-support"))]
use crate::Error;
use crate::Policy;

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
    /// The matter folder is Clauseo's data folder, holds it, or is inside it, or the data folder
    /// doesn't resolve. Holding it, the matter's `Modify` pair would reach `<data_dir>\sandbox`
    /// (the SID map, the ledger, the marker); inside it, the agent would reach Clauseo's state.
    OverlapsDataFolder,
    /// The path is the lawyer's profile folder or a folder above it.
    HoldsTheProfile,
    /// The path is `<data_dir>\sandbox`, holds it or is inside it: the SID map, the ledger and
    /// the marker.
    OverlapsSandboxRecords,
    /// Another matter's folder is inside this matter, or holds it: a deny on one would land in
    /// the other's tree, whatever its DACL.
    NestedMatter,
    /// A name in the path is another spelling of a name to Windows: it ends in `.` or a space,
    /// holds `:` (a stream), or looks like a short 8.3 name.
    OddSpelling,
    /// `<data_dir>\agents`, the agent folder a path is in, the credential link or a Claude store
    /// file is a junction, a symbolic link or another reparse point.
    ReparsePoint,
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
            FolderRefusal::OverlapsDataFolder => {
                f.write_str("it is, holds or is inside Clauseo's data folder")
            }
            FolderRefusal::HoldsTheProfile => f.write_str("it is or holds the profile folder"),
            FolderRefusal::OverlapsSandboxRecords => {
                f.write_str("it is, holds or is inside the sandbox's records folder")
            }
            FolderRefusal::NestedMatter => f.write_str("it is inside this matter, or holds it"),
            FolderRefusal::OddSpelling => f.write_str("a name in it is spelled another way"),
            FolderRefusal::ReparsePoint => {
                f.write_str("it, or the agents' folder it is in, is a link or a reparse point")
            }
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
    data_dir: PathBuf,
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

    /// Clauseo's data folder, canonical.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
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

/// The gate on Windows, with [`folder_support`](super::folder_support): see [`gate`] for its
/// steps. The only way, outside tests, to an [`Admitted`], so to a plan and a ledger entry.
#[cfg(windows)]
pub fn admit<'a>(
    policy: &'a Policy,
    data_dir: &Path,
    others: &[PathBuf],
    groups: &dyn GroupAccess,
) -> Result<Admitted<'a>, Error> {
    gate(policy, data_dir, others, super::folder_support, groups)
}

/// The gate with a fake folder check, for this crate's tests and, through `test-support`,
/// others'.
#[cfg(any(test, feature = "test-support"))]
pub fn admit_with<'a>(
    policy: &'a Policy,
    data_dir: &Path,
    others: &[PathBuf],
    check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    groups: &dyn GroupAccess,
) -> Result<Admitted<'a>, Error> {
    gate(policy, data_dir, others, check, groups)
}

/// The gate, with the folder check `check` and the DACL reader `groups` as parameters.
///
/// 0. The matter folder must not be `data_dir` (Clauseo's data folder), hold it, or sit inside
///    it, compared canonically: `data_dir` is canonicalised (the policy's paths already are),
///    then compared part by part by the policy's comparison and also without ASCII case, as
///    NTFS compares names. A `data_dir` that doesn't resolve refuses, failing closed. Either
///    refuses with [`FolderRefusal::OverlapsDataFolder`] for [`PathKind::Matter`].
/// 1. Each of [`grant_folders`] must answer [`FolderSupport::Ok`]; any other answer, or an
///    error, refuses with [`Error::FolderUnsupported`] naming the folder's kind.
/// 2. Each of `others`, the other matters' folders (their matter, state and notes folders):
///    when `groups` says a group can reach it, or can't tell, it is closed to this matter, and
///    so must answer `Ok` too. A `Network` folder is left alone: `ClauseoSandbox` is a local
///    account a file server doesn't know, and the restricted token was refused every operation
///    over `\\localhost` (`smb-spike.md`). A missing folder is left alone: nothing is there.
///    Any other answer refuses with [`PathKind::OtherMatter`]. Before any of that, another
///    matter's folder inside this matter, or holding it, refuses with
///    [`FolderRefusal::NestedMatter`], whatever its DACL.
///
/// Nothing is written. A refusal returns before anything is planned.
#[cfg(any(windows, test, feature = "test-support"))]
fn gate<'a>(
    policy: &'a Policy,
    data_dir: &Path,
    others: &[PathBuf],
    mut check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    groups: &dyn GroupAccess,
) -> Result<Admitted<'a>, Error> {
    let data = match dunce::canonicalize(data_dir) {
        Ok(data) => data,
        Err(error) => {
            let error = error.kind();
            tracing::warn!(%error, "the data folder doesn't resolve; tier B refuses");
            return Err(refused(PathKind::Matter, FolderRefusal::OverlapsDataFolder));
        }
    };
    let matter = policy.matter();
    if in_folder(matter, &data) || in_folder(&data, matter) {
        return Err(refused(PathKind::Matter, FolderRefusal::OverlapsDataFolder));
    }
    if let Some((kind, refusal)) = agents_links(policy, &data) {
        return Err(refused(kind, refusal));
    }
    if let Some((kind, refusal)) = unresolved_links(policy) {
        return Err(refused(kind, refusal));
    }
    for other in others {
        let canonical = dunce::canonicalize(other).unwrap_or_else(|_missing| other.clone());
        let nested = [other, &canonical]
            .iter()
            .any(|o| in_folder(o, matter) || in_folder(matter, o));
        if nested {
            return Err(refused(PathKind::OtherMatter, FolderRefusal::NestedMatter));
        }
    }
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
    Ok(Admitted {
        policy,
        data_dir: data,
        closed,
    })
}

/// Why the agents' folder can't be trusted, if it can't: `<data_dir>\agents` is a reparse point
/// (a junction could swap what the canonical paths compare against), or an agent folder that
/// holds one of the policy's read, runtime or Claude store paths is. Read with
/// `symlink_metadata`, which opens the name itself (`FILE_FLAG_OPEN_REPARSE_POINT` on Windows)
/// and sees every reparse point there; a read that fails refuses.
#[cfg(any(windows, test, feature = "test-support"))]
fn agents_links(policy: &Policy, data: &Path) -> Option<(PathKind, FolderRefusal)> {
    let agents = data.join(AGENTS_DIR);
    let check = |path: &Path| match is_reparse_point(path) {
        Ok(false) => None,
        Ok(true) => Some(FolderRefusal::ReparsePoint),
        Err(error) => Some(FolderRefusal::CheckFailed(error.kind())),
    };
    if let Some(refusal) = check(&agents) {
        return Some((PathKind::Runtime, refusal));
    }
    let mut paths: Vec<(PathKind, &Path)> = Vec::new();
    paths.extend(
        policy
            .runtime()
            .iter()
            .map(|p| (PathKind::Runtime, p.as_path())),
    );
    paths.extend(policy.read().iter().map(|p| (PathKind::Read, p.as_path())));
    paths.extend(
        policy
            .allowed_folders()
            .iter()
            .map(|p| (PathKind::AllowedFolder, p.as_path())),
    );
    if let Some(store) = policy.claude_store() {
        paths.push((PathKind::ClaudeStore, store.dir()));
    }
    for (kind, path) in paths {
        if !in_folder(path, &agents) {
            continue;
        }
        let depth = agents.components().count() + 1;
        let folder: PathBuf = path.components().take(depth).collect();
        if let Some(refusal) = check(&folder) {
            return Some((kind, refusal));
        }
    }
    None
}

/// Why a name the policy keeps unresolved can't take an entry, if it can't: the credential
/// link and the Claude store's allowed files are read by their own name, never followed, so a
/// symbolic link or a junction there (a reparse point on Windows) would carry the entry to
/// whatever it points at. A hard link is fine: the credential link is one by design (the brief),
/// and shares its file's entries. A missing name is fine: nothing is there to grant on.
#[cfg(any(windows, test, feature = "test-support"))]
fn unresolved_links(policy: &Policy) -> Option<(PathKind, FolderRefusal)> {
    let mut names: Vec<(PathKind, PathBuf)> = Vec::new();
    if let Some(link) = policy.credential_link() {
        names.push((PathKind::CredentialLink, link.to_owned()));
    }
    if let Some(store) = policy.claude_store() {
        names.extend(
            store
                .files()
                .into_iter()
                .map(|f| (PathKind::ClaudeStoreFile, f)),
        );
    }
    names
        .into_iter()
        .find_map(|(kind, name)| match is_reparse_point(&name) {
            Ok(false) => None,
            Ok(true) => Some((kind, FolderRefusal::ReparsePoint)),
            Err(error) => Some((kind, FolderRefusal::CheckFailed(error.kind()))),
        })
}

/// Whether `path` is a reparse point (on Windows any, elsewhere a symbolic link); a missing
/// path is not.
#[cfg(any(windows, test, feature = "test-support"))]
fn is_reparse_point(path: &Path) -> io::Result<bool> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        Ok(meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    Ok(meta.file_type().is_symlink())
}

/// `None` for `Ok`, else why the folder is refused.
#[cfg(any(windows, test, feature = "test-support"))]
fn refusal(answer: io::Result<FolderSupport>) -> Option<FolderRefusal> {
    match answer {
        Ok(FolderSupport::Ok) => None,
        Ok(support) => Some(FolderRefusal::Support(support)),
        Err(error) => Some(FolderRefusal::CheckFailed(error.kind())),
    }
}

/// The refusal, logged by kind only.
#[cfg(any(windows, test, feature = "test-support"))]
fn refused(path_kind: PathKind, support: FolderRefusal) -> Error {
    tracing::info!(%path_kind, %support, "tier B refused a folder; nothing was planned");
    Error::FolderUnsupported { path_kind, support }
}

#[cfg(test)]
mod tests;
