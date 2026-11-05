//! The folder gate (S15's D1): before tier B plans or writes anything for a launch, every folder
//! it would grant on must take permission entries, and so must every sibling folder it will deny
//! the pool users. One refusal refuses the whole launch, and a refusal has planned and written
//! nothing: [`Admitted`] is the only way into [`acl_plan::plan`](super::acl_plan::plan), and only
//! the gate makes one.
//!
//! The gate's logic takes the folder check and the folder lister as parameters ([`admit_with`]),
//! so its tests run on every OS; [`admit`] is the Windows call with the real folder check.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::FolderSupport;
// The gate itself builds on Windows and for tests; elsewhere only its types do.
#[cfg(any(windows, test, feature = "test-support"))]
use super::acl_plan::in_folder;
#[cfg(any(windows, test, feature = "test-support"))]
use super::acl_plan::targets::AGENTS_DIR;
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
    /// Another registered matter's folder.
    OtherMatter,
    /// A folder beside the matter in its parent that isn't a registered matter: denied to every
    /// pool user's reads.
    Sibling,
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
            PathKind::Sibling => "a folder beside the matter",
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
    /// doesn't resolve. Holding it, the matter's `Modify` entries would reach `<data_dir>\sandbox`
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

/// Lists the folders in a folder: the IO the gate needs for the matter's siblings. A trait so
/// tests can fake it; [`ReadDir`] reads the file system.
pub trait FolderList {
    /// The folders directly in `parent` (files left out), as paths.
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>>;
}

/// [`FolderList`] through `std::fs::read_dir`: entries whose own type (not followed) is a
/// folder. Links are listed by `file_type` as links, so they are left out here; the gate skips
/// any reparse point again anyway.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadDir;

impl FolderList for ReadDir {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        let mut folders = Vec::new();
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                folders.push(entry.path());
            }
        }
        folders.sort();
        Ok(folders)
    }
}

/// A launch the gate let through: every folder it grants on answered `Ok`. Holds the policy and
/// the sibling folders the pool users are to be denied reading. Only the gate makes one.
#[derive(Debug)]
pub struct Admitted<'a> {
    policy: &'a Policy,
    data_dir: PathBuf,
    siblings: Vec<PathBuf>,
}

impl<'a> Admitted<'a> {
    /// The policy the gate checked.
    pub fn policy(&self) -> &'a Policy {
        self.policy
    }

    /// The folders beside the matter in its parent the plan denies the pool group reading, each
    /// answering `Ok`: not the matter, not a registered matter, no link, none holding or inside
    /// one of the policy's paths, the profile or the data folder.
    pub fn siblings(&self) -> &[PathBuf] {
        &self.siblings
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
    registered: &[PathBuf],
    folders: &dyn FolderList,
) -> Result<Admitted<'a>, Error> {
    gate(policy, data_dir, registered, super::folder_support, folders)
}

/// The gate with a fake folder check, for this crate's tests and, through `test-support`,
/// others'.
#[cfg(any(test, feature = "test-support"))]
pub fn admit_with<'a>(
    policy: &'a Policy,
    data_dir: &Path,
    registered: &[PathBuf],
    check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    folders: &dyn FolderList,
) -> Result<Admitted<'a>, Error> {
    gate(policy, data_dir, registered, check, folders)
}

/// The gate, with the folder check `check` and the folder lister `folders` as parameters.
///
/// 0. The matter folder must not be `data_dir` (Clauseo's data folder), hold it, or sit inside
///    it, compared canonically: `data_dir` is canonicalised (the policy's paths already are),
///    then compared part by part by the policy's comparison and also without ASCII case, as
///    NTFS compares names. A `data_dir` that doesn't resolve refuses, failing closed. Either
///    refuses with [`FolderRefusal::OverlapsDataFolder`] for [`PathKind::Matter`].
/// 1. Each of [`grant_folders`] must answer [`FolderSupport::Ok`]; any other answer, or an
///    error, refuses with [`Error::FolderUnsupported`] naming the folder's kind.
/// 2. `registered` are the other registered matters' folders (their matter, state and notes
///    folders). One inside this matter, or holding it, refuses with
///    [`FolderRefusal::NestedMatter`] before any folder check. Isolation from them is the plan's
///    deny for each other pool user, on this matter's own folders.
/// 3. The siblings: each folder `folders` lists in the matter's parent, unless the parent is a
///    volume's root (its folders are the system's: `Windows`, `Program Files`, `Users`; a known
///    limit). Left out: the matter itself, a registered matter or a folder holding or inside one,
///    a link or other reparse point, a folder missing by now, and a folder that is, holds or is
///    inside one of the policy's paths, the profile or the data folder (denying it would deny
///    the jail's own reads). Each other one must answer `Ok`, as a grant folder does; `Network`
///    is left alone; any other answer refuses with [`PathKind::Sibling`]. A listing that fails
///    refuses too.
///
/// Nothing is written. A refusal returns before anything is planned.
#[cfg(any(windows, test, feature = "test-support"))]
fn gate<'a>(
    policy: &'a Policy,
    data_dir: &Path,
    registered: &[PathBuf],
    mut check: impl FnMut(&Path) -> io::Result<FolderSupport>,
    folders: &dyn FolderList,
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
    for other in registered {
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
    let siblings = siblings(policy, &data, registered, &mut check, folders)?;
    Ok(Admitted {
        policy,
        data_dir: data,
        siblings,
    })
}

/// Step 3 of [`gate`]: the siblings the plan denies the pool group reading.
#[cfg(any(windows, test, feature = "test-support"))]
fn siblings(
    policy: &Policy,
    data: &Path,
    registered: &[PathBuf],
    check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
    folders: &dyn FolderList,
) -> Result<Vec<PathBuf>, Error> {
    let matter = policy.matter();
    let Some(parent) = matter.parent().filter(|parent| parent.parent().is_some()) else {
        return Ok(Vec::new());
    };
    let listed = folders
        .subfolders(parent)
        .map_err(|error| refused(PathKind::Sibling, FolderRefusal::CheckFailed(error.kind())))?;
    let mut kept: Vec<&Path> = vec![policy.state(), data];
    kept.extend(policy.notes());
    kept.extend(policy.home());
    kept.extend(policy.credential_link());
    kept.extend(policy.claude_store().map(crate::ClaudeStore::dir));
    kept.extend(policy.runtime().iter().map(PathBuf::as_path));
    kept.extend(policy.read().iter().map(PathBuf::as_path));
    kept.extend(policy.allowed_folders().iter().map(PathBuf::as_path));
    kept.extend(registered.iter().map(PathBuf::as_path));
    let overlaps = |a: &Path, b: &Path| in_folder(a, b) || in_folder(b, a);
    let mut siblings = Vec::new();
    for listed in listed {
        let sibling = match dunce::canonicalize(&listed) {
            Ok(sibling) => sibling,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                let refusal = FolderRefusal::CheckFailed(error.kind());
                return Err(refused(PathKind::Sibling, refusal));
            }
        };
        let reparse = is_reparse_point(&listed).map_err(|error| {
            refused(PathKind::Sibling, FolderRefusal::CheckFailed(error.kind()))
        })?;
        if reparse || overlaps(&sibling, matter) || kept.iter().any(|k| overlaps(&sibling, k)) {
            continue;
        }
        match check(&sibling) {
            Ok(FolderSupport::Ok) => siblings.push(sibling),
            Ok(FolderSupport::Network) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            answer => {
                if let Some(refusal) = refusal(answer) {
                    return Err(refused(PathKind::Sibling, refusal));
                }
            }
        }
    }
    Ok(siblings)
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
