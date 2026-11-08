//! Each pool user's own copy of Git for Windows (0049, "The Git tree"): a tree of hard links at
//! `%ProgramFiles%\Clauseo\Sandbox\git\<account>\<tree id>\`, one per pool account.
//!
//! msys2-runtime makes its shared objects under `\Sessions\BNOLINKS\<session>\msys-2.0S5-<key>`,
//! where `<key>` is a hash of the msys install root's path, and a pool user's restricted Low
//! token can't create there; Git Bash, and every `sh.exe` Git starts, then fails. A tree at a
//! path of its own gives each pool account a key of its own, whose directory the outer runner
//! makes for the run (part 2). A junction doesn't change the key (msys resolves it), and
//! `git.exe` finds `sh.exe` from its own install prefix, so the whole tree is linked.
//!
//! [`ensure`] holds the order and the decisions, with no Windows call; every effect goes through
//! [`TreeFs`], which `os.rs` implements and the tests fake. It runs as SYSTEM or elevated, under
//! the set-up lock (the set-up, the refresh), and:
//!
//! 1. checks the admin folder's chain, `admin_copy::check`'s rule 2 ([`TreeFs::admin_dir`]);
//! 2. finds the per-machine Git ([`discover`](discover::discover)): `HKLM\SOFTWARE\GitForWindows`'s
//!    `InstallPath`, else `%ProgramFiles%\Git`, each a local path with no reparse point on the
//!    way, holding `usr\bin\msys-2.0.dll` and `cmd\git.exe`, outside the profiles folder, in a
//!    chain of folders only administrators can change. A per-user Git is refused: its files'
//!    ACLs leave pool users out, and a hard link shares its file's ACL;
//! 3. names the tree ([`tree_id`]) by the Git version and a hash of the two files' identities;
//! 4. per account, keeps a tree already there under the current id, so only a changed Git
//!    rebuilds; else builds it under a temporary name, links every file, checks the two files
//!    are still the ones the id names, and renames it into place;
//! 5. links from a copy in `git\_source\<id>\`, made once, every file when the install is on
//!    another volume, and otherwise each file with several names in the install (NTFS holds a
//!    file to 1023 names, and each tree would add all of them);
//! 6. records each account's tree, or `Git Bash unavailable: <code>`, in
//!    `HKLM\SOFTWARE\Clauseo\Sandbox\GitTrees` ([`TreeFs::write_record`]);
//! 7. once that record is written, deletes an account's older trees when no process runs as
//!    that account ([`TreeFs::active`]; one that can't be told counts as running), every
//!    temporary tree, the folders of accounts no longer in the pool, and older source copies.
//!
//! Nothing here fails the caller: a failure is logged in fixed words and recorded as
//! [`Unavailable`].

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::windows::admin_copy::Found;
use crate::windows::sids::Sid;

mod build;
mod discover;
#[cfg(test)]
mod fake;
#[cfg(windows)]
mod os;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_more;

pub use build::{ensure, remove};
#[cfg(windows)]
pub use os::{WindowsTrees, ensure_for, ensure_from_record, remove_all};

/// The trees' folder in the admin folder.
pub const GIT_DIR: &str = crate::windows::admin_copy::GIT;
/// The cross-volume source copies' folder in [`GIT_DIR`].
pub const SOURCE_DIR: &str = "_source";
/// What a tree being built is named after: `<tree id>.tmp`.
pub const BUILDING: &str = ".tmp";
/// The record's subkey under `HKLM\SOFTWARE\Clauseo\Sandbox`.
pub const RECORD_SUBKEY: &str = "GitTrees";
/// What a record value starts with when the account has no tree.
pub const UNAVAILABLE: &str = "Git Bash unavailable: ";
/// The msys runtime, from the install root.
pub const MSYS_DLL: [&str; 3] = ["usr", "bin", "msys-2.0.dll"];
/// Git, from the install root.
pub const GIT_EXE: [&str; 2] = ["cmd", "git.exe"];

/// `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`.
const READ_EXECUTE: &str = "0x1200a9";
/// `FILE_GENERIC_EXECUTE`: traverse and read attributes, no listing.
const TRAVERSE: &str = "0x1200a0";

/// Why an account has no tree: the record's `Git Bash unavailable: <code>`. Appended only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// `no-git`: no per-machine Git for Windows (none registered, none in `%ProgramFiles%\Git`,
    /// or one without `usr\bin\msys-2.0.dll` or `cmd\git.exe`).
    NoGit,
    /// `per-user-git`: the Git found is in a profile, or in a folder others than administrators
    /// can change.
    PerUserGit,
    /// `network-path`: the source or the admin folder is a network path.
    NetworkPath,
    /// `reparse-point`: a link or junction on the way to the source, or in it.
    ReparsePoint,
    /// `source-unreadable`: the source couldn't be read or listed.
    SourceUnreadable,
    /// `folder-refused`: the admin folder's chain, or a folder of the tree, failed its check or
    /// couldn't be given its owner and DACL.
    FolderRefused,
    /// `build-failed`: a link, a copy or the rename into place failed.
    BuildFailed,
    /// `bad-account`: the account's name can't be a folder name, or it has none.
    BadAccount,
}

impl Unavailable {
    /// The code the record and the Event Log carry.
    pub fn code(self) -> &'static str {
        match self {
            Unavailable::NoGit => "no-git",
            Unavailable::PerUserGit => "per-user-git",
            Unavailable::NetworkPath => "network-path",
            Unavailable::ReparsePoint => "reparse-point",
            Unavailable::SourceUnreadable => "source-unreadable",
            Unavailable::FolderRefused => "folder-refused",
            Unavailable::BuildFailed => "build-failed",
            Unavailable::BadAccount => "bad-account",
        }
    }

    /// The code `code` names; `None` for one this build doesn't know.
    pub fn from_code(code: &str) -> Option<Unavailable> {
        ALL.into_iter().find(|reason| reason.code() == code)
    }
}

/// Every [`Unavailable`], in order.
const ALL: [Unavailable; 8] = [
    Unavailable::NoGit,
    Unavailable::PerUserGit,
    Unavailable::NetworkPath,
    Unavailable::ReparsePoint,
    Unavailable::SourceUnreadable,
    Unavailable::FolderRefused,
    Unavailable::BuildFailed,
    Unavailable::BadAccount,
];

/// One account's record value, as written and as the runner (part 2) reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// The account's current tree.
    Tree(PathBuf),
    /// No tree, and why.
    Unavailable(Unavailable),
}

impl Recorded {
    /// The value's text: the tree's path, or `Git Bash unavailable: <code>`.
    pub fn text(&self) -> String {
        match self {
            Recorded::Tree(path) => path.display().to_string(),
            Recorded::Unavailable(reason) => format!("{UNAVAILABLE}{}", reason.code()),
        }
    }

    /// The value `text` names; an unknown code is `None`.
    pub fn parse(text: &str) -> Option<Recorded> {
        match text.strip_prefix(UNAVAILABLE) {
            Some(code) => Unavailable::from_code(code).map(Recorded::Unavailable),
            None if text.is_empty() => None,
            None => Some(Recorded::Tree(PathBuf::from(text))),
        }
    }
}

/// A pool account a tree is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Its name, which names its folder (`ClauseoSbNN`).
    pub name: String,
    /// Its SID, which its folder's DACL grants.
    pub sid: Sid,
}

/// The identity of a file, as `GetFileInformationByHandleEx` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileId {
    /// The volume's serial number.
    pub volume: u64,
    /// The file's 128-bit ID on that volume.
    pub id: u128,
    /// Its size in bytes.
    pub size: u64,
    /// Its last write time, as a `FILETIME`.
    pub written: u64,
}

/// One entry of the source, from its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path from the root, one part per component.
    pub rel: PathBuf,
    /// A folder, else a regular file.
    pub folder: bool,
    /// A file's 128-bit file ID, which its other names in the install share; 0 for a folder.
    pub id: u128,
}

/// A refused or failed effect: a reparse point met, or anything else with its words for the
/// local log (never the Event Log: they may name a path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// A link, a junction or another reparse point, where none may be.
    Reparse,
    /// Anything else.
    Other(String),
}

/// A failed effect's words, for the local log only.
pub type Failed = String;

/// Every effect of the trees. Paths are full local paths; the real one opens each folder of the
/// trees' own chain relative to its parent's handle, never through a reparse point.
pub trait TreeFs {
    /// The admin folder (`%ProgramFiles%\Clauseo\Sandbox`) once its chain passes
    /// `admin_copy::check`'s rule 2.
    fn admin_dir(&mut self) -> Result<PathBuf, Failed>;
    /// `HKLM\SOFTWARE\GitForWindows`'s `InstallPath` (`REG_SZ`, 64-bit view); `None` when the
    /// key or the value is missing.
    fn registry_install(&mut self) -> Result<Option<String>, Failed>;
    /// `%ProgramFiles%` (`FOLDERID_ProgramFiles`).
    fn program_files(&mut self) -> Result<PathBuf, Failed>;
    /// The profiles folder (`ProfileList`'s `ProfilesDirectory`, expanded).
    fn profiles_dir(&mut self) -> Result<PathBuf, Failed>;
    /// Whether `path` is a network path: UNC, on a remote drive, or with a UNC final path.
    fn is_network(&mut self, path: &Path) -> Result<bool, Failed>;
    /// Opens each folder of `dir` from its volume's root relative to its parent's handle,
    /// refusing a reparse point at any level, `dir` included; a missing one is `Other`.
    fn plain_chain(&mut self, dir: &Path) -> Result<(), Refused>;
    /// Whether the folder `dir`, and every folder above it but the volume root, is one only
    /// SYSTEM, Administrators or `TrustedInstaller` can change (`admin_copy::level_safe`), and
    /// the volume root holds them to the rights that could replace what is in it
    /// (`admin_copy::root_safe`): `admin_copy::check`'s rule 2 for another chain.
    fn admin_only(&mut self, dir: &Path) -> Result<bool, Failed>;
    /// The identity of the regular file `path`, opened as itself; `None` when it isn't there.
    fn file_id(&mut self, path: &Path) -> Result<Option<FileId>, Refused>;
    /// The `FILEVERSION` of the exe `path`, dotted; `None` when it carries none.
    fn file_version(&mut self, path: &Path) -> Result<Option<String>, Failed>;
    /// The serial number of the volume holding `path`.
    fn volume(&mut self, path: &Path) -> Result<u64, Failed>;
    /// Every folder and file below `root`, each folder before what it holds, each file with its
    /// file ID; a reparse point anywhere refuses.
    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused>;
    /// What is at `path`, never following a link.
    fn found(&mut self, path: &Path) -> Result<Found, Failed>;
    /// Makes the folder `path` if missing, opened as itself relative to its parent; refuses a
    /// reparse point or a file there; gives it the owner and protected DACL `sddl` describes
    /// through that handle and reads them back.
    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused>;
    /// The names in the folder `dir`; none when it is missing.
    fn entries(&mut self, dir: &Path) -> Result<Vec<String>, Failed>;
    /// Makes the folder `path`, inheriting its parent's DACL.
    fn make_dir(&mut self, path: &Path) -> Result<(), Failed>;
    /// Makes `link` a new name of the file `existing` (`CreateHardLinkW`).
    fn hard_link(&mut self, existing: &Path, link: &Path) -> Result<(), Failed>;
    /// Copies the file `from` to the new file `to`, which inherits its folder's DACL.
    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), Failed>;
    /// Renames the folder `from` to `to`, which must not exist (write-through).
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), Failed>;
    /// Deletes `path` and all below it, a link as itself, never through one; missing is `Ok`.
    fn remove_tree(&mut self, path: &Path) -> Result<(), Failed>;
    /// Whether any process runs as `account`.
    fn active(&mut self, account: &Sid) -> Result<bool, Failed>;
    /// Replaces the record's subkey with one value per account: its SID's string, `REG_SZ`.
    fn write_record(&mut self, values: &[(Sid, String)]) -> Result<(), Failed>;
    /// Deletes the record's subkey; missing is `Ok`.
    fn delete_record(&mut self) -> Result<(), Failed>;
    /// Writes one line to the log, the Event Log on Windows: fixed words, no path, no name.
    fn log(&mut self, error: bool, line: &str);
}

/// The Git found, with what names its tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Its install root.
    pub root: PathBuf,
    /// Its tree id ([`tree_id`]).
    pub id: String,
    /// The identities of its `msys-2.0.dll` and `git.exe` the id was made from, checked again
    /// before a tree is renamed into place.
    pub files: [FileId; 2],
}

/// The tree id of the Git at `root`, version `version`, from its `msys-2.0.dll` and `git.exe`:
/// `<version>-<12 hex digits>`, the digits the start of a SHA-256 over the root's path (lower
/// case) and each file's volume, ID, size and last write time. A replaced file has another ID,
/// so an update, a repair or a move names a new tree. The version keeps `0-9`, `A-Z`, `a-z` and
/// `.`, at most 24 of them; with none left it is `git`.
pub fn tree_id(root: &Path, version: Option<&str>, msys: &FileId, git: &FileId) -> String {
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().to_lowercase().as_bytes());
    for file in [msys, git] {
        hash.update(file.volume.to_le_bytes());
        hash.update(file.id.to_le_bytes());
        hash.update(file.size.to_le_bytes());
        hash.update(file.written.to_le_bytes());
    }
    let digest = hash.finalize();
    let short = crate::windows::refresh::hex(digest.get(..6).unwrap_or_default());
    let version: String = version
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
        .take(24)
        .collect();
    let version = version.trim_matches('.');
    let version = if version.is_empty() { "git" } else { version };
    format!("{version}-{short}")
}

/// Whether `name` may name an account's folder: 1 to 20 of `0-9`, `A-Z`, `a-z`, `-` and `_`,
/// not starting with `_` (which [`SOURCE_DIR`] does).
pub fn folder_name_ok(name: &str) -> bool {
    (1..=20).contains(&name.len())
        && !name.starts_with('_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The DACL of [`GIT_DIR`]: SYSTEM and Administrators full, the pool's group traverse only.
pub fn git_dir_sddl(group: &Sid) -> String {
    format!("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;{TRAVERSE};;;{group})")
}

/// The DACL of an account's folder and of each of its trees: SYSTEM and Administrators full,
/// the account read and execute, inherited by all below; no one else.
pub fn account_sddl(account: &Sid) -> String {
    format!("O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;{READ_EXECUTE};;;{account})")
}

/// The DACL of [`SOURCE_DIR`] and of each source copy: SYSTEM and Administrators full, the
/// pool's group read and execute, inherited by all below.
pub fn source_sddl(group: &Sid) -> String {
    format!("O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;{READ_EXECUTE};;;{group})")
}

/// `path` joined with each of `parts`.
pub(crate) fn joined(path: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(path.to_path_buf(), |path, part| path.join(part))
}
