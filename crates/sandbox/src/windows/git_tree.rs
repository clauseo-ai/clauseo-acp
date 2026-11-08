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
//!    chain of folders only administrators can change. Every folder and file in it must be one
//!    only administrators can change too (else `source-unsafe`), and none a reparse point. A
//!    per-user Git is refused: its files' ACLs leave pool users out;
//! 3. names the tree ([`tree_id`]) by the Git version, the two files' identities and a digest
//!    of every name, size and write time in the install;
//! 4. copies the install once per tree id into `git\_source\<id>\`, SYSTEM's own files, each
//!    read through a handle opened relative to its walked folder and checked to be the file the
//!    walk saw. The trees link only that copy, never the lawyer's install: the lawyer's Git
//!    processes never hold a tree's file, and a pool process never holds the install's;
//! 5. per account, keeps a tree already there under the current id when its owner is SYSTEM
//!    and its completion marker names the id and the account's SID; else builds it under a
//!    temporary name, links every file of the copy, writes the marker last and renames it
//!    into place;
//! 6. records each account's tree, or `Git Bash unavailable: <code>`, in
//!    `HKLM\SOFTWARE\Clauseo\Sandbox\GitTrees`, one value at a time;
//! 7. once that record is written, deletes an account's trees but the current and the previous
//!    when no process runs as that account ([`TreeFs::active`]; one that can't be told counts
//!    as running), every temporary tree, the folders of accounts no longer in the pool, and
//!    older source copies.
//!
//! Nothing here changes the security of a file below `git\`, and nothing is deleted but by a
//! plain delete (`discard.rs`): what is in use moves to `git\_trash\`, which each run empties.
//! Nothing here fails the caller: a failure is logged in fixed words and recorded as
//! [`Unavailable`].

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::windows::admin_copy::Found;
use crate::windows::sids::Sid;

mod build;
mod discard;
mod discover;
#[cfg(test)]
mod fake;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(windows)]
mod os;
mod retire;
mod stub;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_more;
#[cfg(test)]
mod tests_safe;
#[cfg(test)]
mod tests_stub;

pub use build::{ensure, ensure_with};
#[cfg(windows)]
pub use os::{
    WindowsTrees, check_adopted_here, ensure_for, ensure_from_record, remove_all,
    remove_folder_here, runner_for_stub,
};
pub use retire::{check_adopted, remove};
pub use stub::{OFF_DIR, STUB_NAME, off_sddl};

/// The trees' folder in the admin folder.
pub const GIT_DIR: &str = crate::windows::admin_copy::GIT;
/// The source copies' folder in [`GIT_DIR`]: one copy of the install per tree id.
pub const SOURCE_DIR: &str = "_source";
/// The folder in [`GIT_DIR`] what couldn't be deleted at once waits in, under random names.
pub const TRASH_DIR: &str = "_trash";
/// The completion marker in each tree and source copy, written last ([`marker_text`]).
pub const MARKER: &str = ".clauseo-tree";
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
/// The ACE that lets any signed-in user read a folder's owner and DACL, not inherited:
/// `READ_CONTROL | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, no listing and no file read. The outer
/// runner, the unelevated desktop user, checks each level of a tree through it (part 2).
pub(super) const READ_SECURITY: &str = "(A;;0x120080;;;AU)";

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
    /// `source-unsafe`: a folder or file in the install is one others than administrators can
    /// change.
    SourceUnsafe,
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
            Unavailable::SourceUnsafe => "source-unsafe",
        }
    }

    /// The code `code` names; `None` for one this build doesn't know.
    pub fn from_code(code: &str) -> Option<Unavailable> {
        ALL.into_iter().find(|reason| reason.code() == code)
    }
}

/// Every [`Unavailable`], in order.
const ALL: [Unavailable; 9] = [
    Unavailable::NoGit,
    Unavailable::PerUserGit,
    Unavailable::NetworkPath,
    Unavailable::ReparsePoint,
    Unavailable::SourceUnreadable,
    Unavailable::FolderRefused,
    Unavailable::BuildFailed,
    Unavailable::BadAccount,
    Unavailable::SourceUnsafe,
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

/// One entry of a walked folder, from its root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    /// Its path from the root, one part per component.
    pub rel: PathBuf,
    /// A folder, else a regular file or a reparse point.
    pub folder: bool,
    /// A reparse point (a link, a junction, any tag), listed as itself and never entered.
    pub reparse: bool,
    /// A file's 128-bit file ID, which its other names share; 0 for a folder.
    pub id: u128,
    /// A file's size in bytes; 0 for a folder.
    pub size: u64,
    /// Its last write time, as a `FILETIME`.
    pub written: u64,
    /// Whether only SYSTEM, Administrators or `TrustedInstaller` can change it: its owner and
    /// DACL, read through the handle it was opened by, pass `admin_copy::level_safe`.
    pub admin_only: bool,
}

/// Who owns a folder or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// `S-1-5-18`.
    System,
    /// `S-1-5-32-544`.
    Administrators,
    /// Anyone else.
    Other,
}

/// What is at a path, read through a handle opened on it as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inspected {
    /// A folder, a file or a link (never `Missing`).
    pub kind: Found,
    /// Its owner.
    pub owner: Owner,
    /// Whether its owner and DACL pass `admin_copy::level_safe`.
    pub admin_only: bool,
    /// When it was made, as a `FILETIME` (a rename keeps it).
    pub created: u64,
}

/// A refused or failed effect: a reparse point met, or anything else with its words for the
/// local log (never the Event Log: they may name a path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// A link, a junction or another reparse point, where none may be.
    Reparse,
    /// Anything else.
    Other(String),
    /// A file opened by its name isn't the one the walk saw: another file ID, size or write time.
    Changed,
}

/// A failed effect's words, for the local log only.
pub type Failed = String;

/// Every effect of the trees. Paths are full local paths; the real one opens each folder of the
/// trees' own chain relative to its parent's handle, never through a reparse point, and never
/// changes the security of anything but a folder [`TreeFs::level`] is given.
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
    /// Every folder, file and reparse point below the folder `root` (not `root` itself), each
    /// folder before what it holds. Each is opened relative to its walked folder's handle as
    /// itself, never following a link, and described from that handle (its file ID, size,
    /// write time, and owner and DACL for [`Entry::admin_only`]); a reparse point is listed and
    /// never entered. `root` a reparse point is [`Refused::Reparse`].
    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused>;
    /// What is at `path`, never following a link.
    fn found(&mut self, path: &Path) -> Result<Found, Failed>;
    /// What is at `path`, opened as itself relative to its parent's handle; `None` when
    /// nothing is there.
    fn inspect(&mut self, path: &Path) -> Result<Option<Inspected>, Failed>;
    /// Makes the folder `path` if missing, opened as itself relative to its parent; refuses a
    /// reparse point or a file there; gives it the owner and protected DACL `sddl` describes
    /// through that handle, which never propagates below, and reads them back.
    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused>;
    /// The names in the folder `dir`; none when it is missing.
    fn entries(&mut self, dir: &Path) -> Result<Vec<String>, Failed>;
    /// Into the empty folder `to`, `entries` of the folder `from` (a walk's, in its order):
    /// each folder made relative to its parent's handle, owner SYSTEM, its DACL inherited; each
    /// file opened relative to its walked folder's handle as itself, refused unless its file
    /// ID, size and write time are the entry's ([`Refused::Changed`]), and its data copied
    /// through that handle into a new file made relative to its folder's handle, owner SYSTEM,
    /// its DACL inherited. A reparse point is [`Refused::Reparse`].
    fn copy_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused>;
    /// [`TreeFs::copy_tree`], but each file is linked, not copied: opened as itself relative to
    /// its walked folder's handle, its file ID checked, and given a new name relative to the
    /// destination folder's handle (`FileLinkInformation`), sharing its security.
    fn link_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused>;
    /// The text of [`MARKER`] in the folder `dir`, a plain file with one name opened relative
    /// to `dir`'s handle; `None` when it is missing.
    fn read_marker(&mut self, dir: &Path) -> Result<Option<String>, Failed>;
    /// Makes [`MARKER`] in the folder `dir`, relative to its handle, with the owner and
    /// protected DACL `sddl` describes ([`marker_sddl`]), holding `text`, written through.
    fn write_marker(&mut self, dir: &Path, text: &str, sddl: &str) -> Result<(), Failed>;
    /// Renames `from` to `to`, which must not exist (write-through).
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), Failed>;
    /// Deletes `path` and all below it by a plain delete: each entry opened relative to its
    /// folder's handle as itself, a link deleted as itself and never followed, nothing's
    /// security changed. Goes on past what it can't delete (a file in use), then fails;
    /// missing is `Ok`.
    fn remove_tree(&mut self, path: &Path) -> Result<(), Failed>;
    /// Schedules `path` and everything below it for deletion at the next restart
    /// (`MOVEFILE_DELAY_UNTIL_REBOOT`), the deepest first.
    fn delete_at_reboot(&mut self, path: &Path) -> Result<(), Failed>;
    /// Copies the regular file `from` (its folder's chain opened plainly, the file opened as
    /// itself relative to its folder's handle, no reparse point) into the folder `dir` as
    /// `temp`, made relative to its handle, owner SYSTEM, its DACL inherited, written through;
    /// then renames it to `name` relative to the same handle, replacing a file there. A failure
    /// deletes `temp`.
    fn place_file(
        &mut self,
        from: &Path,
        dir: &Path,
        temp: &str,
        name: &str,
    ) -> Result<(), Refused>;
    /// A random name for the trash: 16 lower-case hex digits.
    fn unique(&mut self) -> String;
    /// Whether any process runs as `account`.
    fn active(&mut self, account: &Sid) -> Result<bool, Failed>;
    /// Sets the record subkey's value named by `account`'s SID to `text` (`REG_SZ`), one
    /// write that replaces it whole; makes the subkey when missing.
    fn set_record_value(&mut self, account: &Sid, text: &str) -> Result<(), Failed>;
    /// The names of the record subkey's values; none when it is missing.
    fn record_names(&mut self) -> Result<Vec<String>, Failed>;
    /// Deletes the record subkey's value `name`; missing is `Ok`.
    fn delete_record_value(&mut self, name: &str) -> Result<(), Failed>;
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
    /// before its copy is renamed into place.
    pub files: [FileId; 2],
    /// Its walk, which the id's digest covers and its copy follows, without a [`MARKER`] at
    /// its root.
    pub listing: Vec<Entry>,
}

/// The tree id of the Git at `root`, version `version`, from its `msys-2.0.dll` and `git.exe`
/// and its listing's [`digest`]: `<version>-<12 hex digits>`, the digits the start of a SHA-256
/// over the root's path (lower case), each file's volume, ID, size and last write time, and the
/// digest. A replaced file has another ID, and a repair that swaps any other file changes the
/// digest, so an update, a repair or a move names a new tree. The version keeps `0-9`, `A-Z`,
/// `a-z` and `.`, at most 24 of them; with none left it is `git`.
pub fn tree_id(
    root: &Path,
    version: Option<&str>,
    files: [&FileId; 2],
    listing: &[u8; 32],
) -> String {
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().to_lowercase().as_bytes());
    for file in files {
        hash.update(file.volume.to_le_bytes());
        hash.update(file.id.to_le_bytes());
        hash.update(file.size.to_le_bytes());
        hash.update(file.written.to_le_bytes());
    }
    hash.update(listing);
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

/// The digest of a walk's `entries`: a SHA-256 over each one's path (lower case), kind, size
/// and last write time, in order. Metadata only: no file is read.
pub fn digest(entries: &[Entry]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for entry in entries {
        let rel = entry.rel.to_string_lossy().to_lowercase();
        hash.update(u64::try_from(rel.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(rel.as_bytes());
        hash.update([u8::from(entry.folder), u8::from(entry.reparse)]);
        hash.update(entry.size.to_le_bytes());
        hash.update(entry.written.to_le_bytes());
    }
    hash.finalize().into()
}

/// The text of a completion marker, as the outer runner (part 2) reads it: `<tree id>\n`, then
/// for an account's tree `<SID>\n`.
pub fn marker_text(id: &str, account: Option<&Sid>) -> String {
    match account {
        Some(sid) => format!("{id}\n{sid}\n"),
        None => format!("{id}\n"),
    }
}

/// Whether the marker text `found` is `wanted` ([`marker_text`]): line by line, a `\r` before
/// each `\n` accepted, the tree id compared without case, the SID exactly.
pub fn marker_matches(found: &str, wanted: &str) -> bool {
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect()
    };
    let (found, wanted) = (lines(found), lines(wanted));
    found.len() == wanted.len()
        && found.iter().zip(&wanted).enumerate().all(|(at, (a, b))| {
            if at == 0 {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        })
}

/// The descriptor of a completion marker: owner SYSTEM, a protected DACL with SYSTEM and
/// Administrators full, an account's tree's account read and execute, and Authenticated Users
/// `FILE_GENERIC_READ` (`0x120089`), so the unelevated outer runner can read it (part 2).
pub fn marker_sddl(account: Option<&Sid>) -> String {
    let reader = account
        .map(|sid| format!("(A;;{READ_EXECUTE};;;{sid})"))
        .unwrap_or_default();
    format!("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA){reader}(A;;0x120089;;;AU)")
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

/// The DACL of [`GIT_DIR`]: SYSTEM and Administrators full, the pool's group traverse only,
/// and any signed-in user its security alone ([`READ_SECURITY`]).
pub fn git_dir_sddl(group: &Sid) -> String {
    format!("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;{TRAVERSE};;;{group}){READ_SECURITY}")
}

/// The DACL of an account's folder and of each of its trees: SYSTEM and Administrators full,
/// the account read and execute, inherited by all below; any signed-in user the folder's own
/// security alone ([`READ_SECURITY`], not inherited); no one else.
pub fn account_sddl(account: &Sid) -> String {
    format!(
        "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;{READ_EXECUTE};;;{account}){READ_SECURITY}"
    )
}

/// The DACL of [`SOURCE_DIR`] and of each source copy: SYSTEM and Administrators full, the
/// pool's group read and execute, inherited by all below; and Users read and execute on every
/// file below (inherit-only, object inherit), as Git's own install files have: a tree's files
/// are links of these, sharing their descriptor, and the unelevated outer runner reads a
/// tree's `msys-2.0.dll` (part 2). Git's content isn't secret.
pub fn source_sddl(group: &Sid) -> String {
    format!(
        "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;{READ_EXECUTE};;;{group})\
         (A;OIIO;{READ_EXECUTE};;;BU)"
    )
}

/// The DACL of [`TRASH_DIR`]: SYSTEM and Administrators full, inherited; no one else.
pub const TRASH_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// Whether `a` and `b` are the same path, ignoring case and the separator.
pub(crate) fn same(a: &Path, b: &Path) -> bool {
    let text = |path: &Path| path.to_string_lossy().replace('/', "\\").to_lowercase();
    text(a).trim_end_matches('\\') == text(b).trim_end_matches('\\')
}

/// `path` joined with each of `parts`.
pub(crate) fn joined(path: &Path, parts: &[&str]) -> PathBuf {
    parts
        .iter()
        .fold(path.to_path_buf(), |path, part| path.join(part))
}
