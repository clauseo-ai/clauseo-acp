//! The set-up program's admin copy (S15 tier B, "Admin copy" in 0049). The installer places
//! `clauseo-sandbox-setup.exe` in `%ProgramFiles%\Clauseo\Sandbox\`, a folder only
//! administrators can change, and records the copy's SHA-256 in the machine's record
//! (`AdminCopySha256`). Clauseo starts only that copy elevated, never the one beside
//! `Clauseo.exe`, which a per-user install keeps in a folder the lawyer's own processes can
//! write.
//!
//! Before each elevated start (`UserSetup`) and each tier-B launch (`check_launch`),
//! [`check`] holds the copy to three rules, in this order:
//!
//! 1. the copy is there as a regular file, not a link or other reparse point, and the record
//!    holds a hash: else [`Blocked::AdminCopyMissing`];
//! 2. each of `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy, and each folder above
//!    `%ProgramFiles%` up to the volume root, is not a reparse point, is owned by SYSTEM,
//!    Administrators or `TrustedInstaller`, and no allow entry that applies to it gives any
//!    other account write, append, delete (of it or of a child), `WRITE_DAC`, `WRITE_OWNER` or
//!    a generic right that holds one of those ([`level_safe`]); the volume root itself, where
//!    every account may add a folder, is held only to the rights that could replace what is in
//!    it ([`root_safe`]): else [`Blocked::AdminCopyUnsafe`]. Deny entries don't matter:
//!    they only take rights away. The admin folder holds nothing but the copy, its signature and
//!    the staging folder ([`expected_entry`]): a DLL planted beside the copy is refused too;
//! 3. the copy's SHA-256 is the recorded one: else [`Blocked::AdminCopyChanged`].
//!
//! The folder check comes before the hash: a copy in a folder others can change proves nothing
//! by its hash, and naming the folder tells IT what to fix. `%ProgramFiles%` comes from
//! `FOLDERID_ProgramFiles`, never from the environment. Every check reads through
//! [`AdminCopyFs`], so the rules are tested on every OS; `os.rs` is the Windows one.
//!
//! An elevated start goes through [`launch_checked`]: the copy is hashed through a handle that
//! shares read only, and that handle stays open until the start has returned, so the file
//! started is the file hashed: nobody can write, rename or delete it in between.

use std::io;
use std::path::{Path, PathBuf};

use super::acl_bytes::{self, Acl, Descriptor};
use super::setup::record_values::CopyOwner;
use super::sids::Sid;
use crate::{Blocked, SetupState};

#[cfg(test)]
mod chain_tests;
#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod launch_tests;
#[cfg(windows)]
pub(crate) mod mark_os;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(windows)]
pub(crate) mod os;
mod security_block;
#[cfg(test)]
mod tests;
mod version;

pub use security_block::launch_recorded;
pub use version::{compare_versions, parse_version, version_not_older};

/// The admin copy's folder under `%ProgramFiles%`, one part at a time.
pub const ADMIN_DIR: [&str; 2] = ["Clauseo", "Sandbox"];
/// The set-up program's file name, the same in the admin folder as beside the app.
pub const SETUP_EXE: &str = "clauseo-sandbox-setup.exe";
/// The copy's detached signature, which the refresh task (S15 tier B PR 3) may keep beside it.
pub const SETUP_SIG: &str = "clauseo-sandbox-setup.exe.sig";
/// The refresh task's staging folder in the admin folder (PR 3).
pub const STAGING: &str = "staging";
/// This build's version: `tauri.conf.json`'s `version` (`build.rs`), the exe's `FileVersion`,
/// recorded as `AdminCopyVersion`.
pub const APP_VERSION: &str = env!("CLAUSEO_APP_VERSION");

/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";
/// `NT SERVICE\TrustedInstaller`.
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// The accounts that may own, or be able to change, a level of the admin copy's chain.
pub const TRUSTED: [&str; 3] = [SYSTEM, ADMINISTRATORS, TRUSTED_INSTALLER];

/// `FILE_WRITE_DATA` (`FILE_ADD_FILE` on a folder).
const WRITE_DATA: u32 = 0x2;
/// `FILE_APPEND_DATA` (`FILE_ADD_SUBDIRECTORY` on a folder).
const APPEND_DATA: u32 = 0x4;
/// `FILE_DELETE_CHILD`.
const DELETE_CHILD: u32 = 0x40;
/// `FILE_WRITE_ATTRIBUTES`.
const WRITE_ATTRIBUTES: u32 = 0x100;
/// `DELETE`.
const DELETE: u32 = 0x1_0000;
/// `WRITE_DAC`.
const WRITE_DAC: u32 = 0x4_0000;
/// `WRITE_OWNER`.
const WRITE_OWNER: u32 = 0x8_0000;
/// `MAXIMUM_ALLOWED`, meaningless in an entry; counted as a change right to be safe.
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
/// `GENERIC_ALL`: `FILE_ALL_ACCESS`.
const GENERIC_ALL: u32 = 0x1000_0000;
/// `GENERIC_WRITE`: `FILE_GENERIC_WRITE`, which holds write and append.
const GENERIC_WRITE: u32 = 0x4000_0000;
/// Every right that lets an account replace, change or re-own a level, generic ones included.
pub const CHANGE_RIGHTS: u32 = WRITE_DATA
    | APPEND_DATA
    | DELETE_CHILD
    | WRITE_ATTRIBUTES
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | MAXIMUM_ALLOWED
    | GENERIC_ALL
    | GENERIC_WRITE;
/// The rights that could replace what is in a volume root: delete a child, or give oneself that
/// (`WRITE_DAC`, `WRITE_OWNER`, the generic ones that hold them). Adding a file or folder there,
/// which every account may, and deleting the root itself, which nobody can, change nothing.
pub const ROOT_CHANGE_RIGHTS: u32 =
    DELETE_CHILD | WRITE_DAC | WRITE_OWNER | MAXIMUM_ALLOWED | GENERIC_ALL;

/// `FILE_ALL_ACCESS`.
pub const FULL_ACCESS: u32 = 0x1F_01FF;
/// `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`: read and run.
pub const READ_EXECUTE: u32 = 0x12_00A9;

/// The admin folder as the installer's `--adopt` leaves it: owner Administrators, a protected
/// DACL, full control for SYSTEM and Administrators and read and run for Users, each inherited
/// by files and folders inside.
pub const LOCKED_FOLDER_SDDL: &str =
    "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";
/// The admin copy itself: the folder's entries, written on the file and protected, so it never
/// depends on what it would inherit.
pub const LOCKED_FILE_SDDL: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)";
/// `BUILTIN\Users`.
const USERS: &str = "S-1-5-32-545";

/// What is at a path, read without following a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// Nothing.
    Missing,
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// A link, a junction or another reparse point, whatever it points at.
    Link,
}

/// What [`check`] reads. `os::WindowsAdminCopy` is the machine; the tests have fakes.
pub trait AdminCopyFs {
    /// `%ProgramFiles%`, from `FOLDERID_ProgramFiles`.
    fn program_files(&self) -> io::Result<PathBuf>;
    /// What is at `path`, never following a link.
    fn found(&self, path: &Path) -> io::Result<Found>;
    /// `path`'s self-relative security descriptor with its owner and DACL, read from the name
    /// itself, never what a link points at.
    fn security(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// The SHA-256 of the regular file at `path`, as 64 lowercase hex digits; refused when
    /// `path` is a reparse point.
    fn sha256(&self, path: &Path) -> io::Result<String>;
    /// As [`AdminCopyFs::sha256`], read through a handle the [`Held`] keeps open, sharing read
    /// only (no write, no delete), so the file can't be changed, renamed or deleted until it
    /// drops.
    fn sha256_held(&self, path: &Path) -> io::Result<(String, Held)>;
    /// The record's `AdminCopySha256`, when it holds one.
    fn recorded_sha256(&self) -> io::Result<Option<String>>;
    /// The names of what is in the folder `dir`.
    fn entries(&self, dir: &Path) -> io::Result<Vec<String>>;
    /// Whether the record names another Windows user than this process's as the pool's owner
    /// (`OwnerSid`) or as the admin copy's (`AdminCopyOwner`, which an in-app removal keeps).
    fn another_owner(&self) -> io::Result<bool>;
    /// Whether this user's `SecurityBlockedAt` mark is there (`security_block`).
    fn security_blocked(&self) -> io::Result<bool>;
    /// Records the mark (`true`, at the time now) or deletes it (`false`).
    fn set_security_blocked(&self, blocked: bool) -> io::Result<()>;
}

/// What keeps a hashed admin copy unchanged ([`AdminCopyFs::sha256_held`]): on Windows the
/// handle it was hashed through. Dropping it lets the file change again.
pub struct Held {
    _keeps: Box<dyn std::any::Any>,
}

impl Held {
    /// A hold that lasts as long as `keeps`.
    pub fn new(keeps: impl std::any::Any) -> Held {
        Held {
            _keeps: Box::new(keeps),
        }
    }
}

/// What a tier-B launch (`check_launch`) and every elevated start (`UserSetup`) check first:
/// a pool owned by another Windows user is [`Blocked::OtherAccount`] (D5's one owner), then
/// [`check`]. A record whose owners can't be read fails closed, as a recorded hash that can't
/// be read does: [`Blocked::AdminCopyMissing`], never "no other owner".
pub fn check_launchable(fs: &dyn AdminCopyFs) -> Result<PathBuf, Blocked> {
    owner_check(fs)?;
    check(fs)
}

/// [`check_launchable`] for an elevated start: the copy is hashed through
/// [`AdminCopyFs::sha256_held`], then `start` is given its path while that handle is still
/// open, and the handle closes only once `start` has returned, with the process started (or
/// not). Nothing starts when a check fails.
pub fn launch_checked<T>(
    fs: &dyn AdminCopyFs,
    start: impl FnOnce(&Path) -> T,
) -> Result<T, Blocked> {
    owner_check(fs)?;
    let (exe, held) = checked(fs, |exe| fs.sha256_held(exe))?;
    let started = start(&exe);
    drop(held);
    Ok(started)
}

/// The record's owners: another Windows user's is [`Blocked::OtherAccount`]; owners that can't
/// be read are [`Blocked::AdminCopyMissing`], as the copy's own record values are.
fn owner_check(fs: &dyn AdminCopyFs) -> Result<(), Blocked> {
    match fs.another_owner() {
        Ok(false) => Ok(()),
        Ok(true) => {
            tracing::warn!("tier B is set up for another account on this computer");
            Err(Blocked::OtherAccount)
        }
        Err(error) => {
            tracing::warn!(%error, "the record's owner can't be read");
            Err(Blocked::AdminCopyMissing)
        }
    }
}

/// Whether `name` may be in the admin folder: the copy, its signature, the staging folder.
/// `--adopt` deletes anything else there, and [`check`] refuses while anything else is.
pub fn expected_entry(name: &str) -> bool {
    [SETUP_EXE, SETUP_SIG, STAGING]
        .iter()
        .any(|expected| name.eq_ignore_ascii_case(expected))
}

/// The admin folder under `program_files`.
pub fn admin_dir(program_files: &Path) -> PathBuf {
    ADMIN_DIR
        .iter()
        .fold(program_files.to_path_buf(), |path, part| path.join(part))
}

/// The admin copy under `program_files`.
pub fn admin_exe(program_files: &Path) -> PathBuf {
    admin_dir(program_files).join(SETUP_EXE)
}

/// The admin copy's path once it passes the module's three rules, else the first rule it
/// breaks. The path is the one to start: nothing else is ever started elevated.
pub fn check(fs: &dyn AdminCopyFs) -> Result<PathBuf, Blocked> {
    checked(fs, |exe| fs.sha256(exe).map(|hash| (hash, ()))).map(|(exe, ())| exe)
}

/// [`check`], the copy hashed by `hash`, which also gives what it holds for the caller.
fn checked<H>(
    fs: &dyn AdminCopyFs,
    hash: impl FnOnce(&Path) -> io::Result<(String, H)>,
) -> Result<(PathBuf, H), Blocked> {
    let program_files = fs.program_files().map_err(|error| {
        tracing::warn!(%error, "Windows gave no Program Files folder");
        Blocked::AdminCopyMissing
    })?;
    let exe = admin_exe(&program_files);
    if !matches!(fs.found(&exe), Ok(Found::File)) {
        tracing::warn!("the set-up program's admin copy isn't there as a regular file");
        return Err(security_block::missing(fs));
    }
    let recorded = match fs.recorded_sha256() {
        Ok(Some(recorded)) => recorded,
        Ok(None) => {
            tracing::warn!("the record holds no hash for the admin copy");
            return Err(Blocked::AdminCopyMissing);
        }
        Err(error) => {
            tracing::warn!(%error, "the admin copy's recorded hash can't be read");
            return Err(Blocked::AdminCopyMissing);
        }
    };
    check_chain(fs, &program_files, &exe)?;
    match fs.entries(&admin_dir(&program_files)) {
        Ok(names) if names.iter().all(|name| expected_entry(name)) => {}
        Ok(_) => {
            tracing::warn!("the admin folder holds something besides the copy");
            return Err(Blocked::AdminCopyUnsafe);
        }
        Err(error) => {
            tracing::warn!(%error, "the admin folder can't be listed");
            return Err(Blocked::AdminCopyUnsafe);
        }
    }
    if !is_sha256_hex(&recorded) {
        tracing::warn!("the admin copy's recorded hash isn't a SHA-256");
        return Err(Blocked::AdminCopyChanged);
    }
    match hash(&exe) {
        Ok((actual, held)) if actual == recorded => {
            security_block::verified(fs);
            Ok((exe, held))
        }
        Ok(_) => {
            tracing::warn!("the admin copy's SHA-256 isn't the recorded one");
            Err(Blocked::AdminCopyChanged)
        }
        Err(error) => {
            tracing::warn!(%error, "the admin copy can't be read to hash it");
            Err(Blocked::AdminCopyChanged)
        }
    }
}

/// Rule 2 from the volume root down: the root, any folder between it and `%ProgramFiles%`, then
/// `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy; each a folder (the copy a file) that is
/// no link, the root with a [`root_safe`] owner and DACL, every other level [`level_safe`]. A
/// `%ProgramFiles%` that isn't on a drive letter (`above` gives nothing) is unsafe.
fn check_chain(fs: &dyn AdminCopyFs, program_files: &Path, exe: &Path) -> Result<(), Blocked> {
    let Some(above) = above(program_files) else {
        tracing::warn!("Program Files isn't a folder on a drive letter");
        return Err(Blocked::AdminCopyUnsafe);
    };
    let mut levels: Vec<(PathBuf, Found)> = above
        .into_iter()
        .map(|path| (path, Found::Folder))
        .collect();
    levels.push((program_files.to_path_buf(), Found::Folder));
    let mut at = program_files.to_path_buf();
    for part in ADMIN_DIR {
        at = at.join(part);
        levels.push((at.clone(), Found::Folder));
    }
    levels.push((exe.to_path_buf(), Found::File));
    for (depth, (path, want)) in levels.iter().enumerate() {
        let found = fs.found(path);
        let rule = if depth == 0 { root_safe } else { level_safe };
        let secured =
            found.as_ref().ok() == Some(want) && fs.security(path).is_ok_and(|bytes| rule(&bytes));
        if !secured {
            tracing::warn!(
                depth,
                ?found,
                "a level of the admin copy's folders isn't safe"
            );
            return Err(Blocked::AdminCopyUnsafe);
        }
    }
    Ok(())
}

/// The folders above `program_files`, from its volume root down: `C:\` alone for
/// `C:\Program Files`. `None` unless it is a path on a drive letter, below its root, with no
/// `.` or `..` part.
pub fn above(program_files: &Path) -> Option<Vec<PathBuf>> {
    let text = normal(program_files);
    let mut parts = text.split('\\');
    let drive = parts.next()?;
    let lettered = drive.len() == 2
        && drive.starts_with(|c: char| c.is_ascii_alphabetic())
        && drive.ends_with(':');
    let rest: Vec<&str> = parts.collect();
    let plain = rest
        .iter()
        .all(|part| !part.is_empty() && *part != "." && *part != "..");
    let (_last, between) = rest.split_last()?;
    if !lettered || !plain {
        return None;
    }
    let mut at = format!("{drive}\\");
    let mut folders = vec![PathBuf::from(&at)];
    for part in between {
        at.push_str(part);
        folders.push(PathBuf::from(&at));
        at.push('\\');
    }
    Some(folders)
}

/// Whether the self-relative descriptor `bytes` keeps its object to [`TRUSTED`] accounts: an
/// owner among them, a DACL (none would allow everyone everything), and no allow entry that
/// applies to the object itself (inherit-only ones don't) giving any [`CHANGE_RIGHTS`] to
/// another SID. An allow entry whose SID can't be read (object and compound entries) counts as
/// another SID's. Deny, audit and label entries are skipped.
pub fn level_safe(bytes: &[u8]) -> bool {
    safe_against(bytes, CHANGE_RIGHTS)
}

/// [`level_safe`] for a volume root: only [`ROOT_CHANGE_RIGHTS`] count, so the entry every
/// Windows install has there (Authenticated Users may add a folder) passes.
pub fn root_safe(bytes: &[u8]) -> bool {
    safe_against(bytes, ROOT_CHANGE_RIGHTS)
}

/// [`level_safe`] with `rights` as the rights that count.
fn safe_against(bytes: &[u8], rights: u32) -> bool {
    let trusted = |sid: &str| TRUSTED.contains(&sid);
    let Some(owner) = acl_bytes::owner_sid(bytes) else {
        return false;
    };
    if !trusted(owner.as_str()) {
        return false;
    }
    let Ok(Descriptor {
        dacl: Some(dacl), ..
    }) = Descriptor::parse(bytes)
    else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    dacl.aces().iter().all(|ace| {
        let kind = acl_bytes::kind(ace);
        let allows = matches!(kind, 0 | 4 | 5 | 9 | 11);
        if !allows || acl_bytes::flags(ace) & acl_bytes::INHERIT_ONLY != 0 {
            return true;
        }
        let Some(mask) = acl_bytes::mask(ace) else {
            return false;
        };
        if mask & rights == 0 {
            return true;
        }
        acl_bytes::sid(ace)
            .and_then(acl_bytes::sid_text)
            .is_some_and(|sid| trusted(sid.as_str()))
    })
}

/// Whether the self-relative descriptor `bytes` is exactly [`LOCKED_FOLDER_SDDL`]'s (`folder`)
/// or [`LOCKED_FILE_SDDL`]'s: owner Administrators, a protected DACL, and its three allow
/// entries with those masks and inheritance and nothing else, in any order. `--adopt` reads the
/// folder and the copy back and refuses on anything else.
pub fn locked(bytes: &[u8], folder: bool) -> bool {
    let inherit = if folder {
        acl_bytes::OBJECT_INHERIT | acl_bytes::CONTAINER_INHERIT
    } else {
        0
    };
    let owner = acl_bytes::owner_sid(bytes);
    if owner.as_ref().map(super::sids::Sid::as_str) != Some(ADMINISTRATORS) {
        return false;
    }
    let Ok(descriptor) = Descriptor::parse(bytes) else {
        return false;
    };
    let Some(dacl) = descriptor.dacl.filter(|_| descriptor.dacl_protected()) else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    let mut seen: Vec<(String, u32)> = Vec::new();
    for ace in dacl.aces() {
        let sid = acl_bytes::sid(ace).and_then(acl_bytes::sid_text);
        let (Some(sid), Some(mask)) = (sid, acl_bytes::mask(ace)) else {
            return false;
        };
        if acl_bytes::kind(ace) != acl_bytes::ALLOWED || acl_bytes::flags(ace) != inherit {
            return false;
        }
        seen.push((sid.as_str().to_owned(), mask));
    }
    seen.sort();
    let mut want = vec![
        (SYSTEM.to_owned(), FULL_ACCESS),
        (ADMINISTRATORS.to_owned(), FULL_ACCESS),
        (USERS.to_owned(), READ_EXECUTE),
    ];
    want.sort();
    seen == want
}

/// Whether `text` is a SHA-256 as the record holds it: 64 lowercase hex digits.
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _infallible = write!(out, "{byte:02x}");
        out
    })
}

/// A Windows path's text as compared here: `/` read as `\`, a `\\?\` prefix (not `\\?\UNC\`)
/// dropped, and no trailing `\` but a root's.
fn normal(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let trimmed = text.trim_end_matches('\\');
    if trimmed.len() <= 2 {
        // `C:` or `C:\`: the root keeps its separator.
        return format!("{trimmed}\\");
    }
    trimmed.to_owned()
}

/// Whether `a` and `b` name the same path as Windows reads them, letter case ignored (ASCII
/// only, so a difference in another script counts as different: the safe side).
pub fn same_path(a: &Path, b: &Path) -> bool {
    normal(a).eq_ignore_ascii_case(&normal(b))
}

/// Whether `child` is strictly inside `parent`, part by part, letter case ignored as
/// [`same_path`] does. A `.` or `..` part in either is never inside.
pub fn inside(child: &Path, parent: &Path) -> bool {
    let (child, parent) = (normal(child), normal(parent));
    let dotted = |text: &str| text.split('\\').any(|part| part == "." || part == "..");
    if dotted(&child) || dotted(&parent) {
        return false;
    }
    let base = parent.trim_end_matches('\\');
    child.len() > base.len() + 1
        && child
            .get(..base.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(base))
        && child
            .get(base.len()..)
            .is_some_and(|tail| tail.starts_with('\\'))
}

/// The protection state the app shows (`UserSetup::state`), with the admin copy first, so the
/// repair shows before any set-up is tried: a copy that fails [`check`] is its cause
/// (`Blocked::AdminCopyMissing` after a declined or silent install, say). With the copy fine,
/// `combined` (the marker and record's state with the last set-up's result, `marker` without
/// it) stands, except that a recorded admin-copy cause the copy no longer has gives way to
/// `marker`, and a pool to set up (`Needed`) whose copy another user's install placed is
/// `Blocked::OtherAccount` (`Blocked::AdminCopyMissing` when the owners can't be read).
pub fn protection_state(
    fs: &dyn AdminCopyFs,
    marker: SetupState,
    combined: SetupState,
) -> SetupState {
    if let Err(blocked) = check(fs) {
        return SetupState::Blocked(blocked);
    }
    let stale = matches!(
        combined,
        SetupState::Blocked(
            Blocked::AdminCopyMissing
                | Blocked::AdminCopyChanged
                | Blocked::AdminCopyUnsafe
                | Blocked::ToolMissing
        )
    );
    let state = if stale { marker } else { combined };
    if state == SetupState::Needed
        && let Err(blocked) = owner_check(fs)
    {
        return SetupState::Blocked(blocked);
    }
    state
}

/// Which `--remove` ran, as [`copy_removable`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removal<'a> {
    /// `--uninstall`: the uninstaller's hook. The app's in-app removal never passes it.
    pub uninstall: bool,
    /// `--per-machine`.
    pub per_machine: bool,
    /// The user who asked, when found.
    pub invoker: Option<&'a Sid>,
}

/// Whether `--remove` deletes the admin copy and its record values: only the uninstaller's
/// (`--uninstall`; the app's plain removal keeps them, so a later set-up runs the verified copy
/// with no installer), only once the pool has no owner left (`pool_owned` false), and only for
/// the install that placed it (one owner, of one kind): a per-machine removal for
/// [`CopyOwner::Machine`], the lawyer's own per-user removal for theirs. A copy no one is
/// recorded as owning stays: it is inert, and only administrators can change it.
pub fn copy_removable(
    removal: &Removal<'_>,
    pool_owned: bool,
    copy_owner: Option<&CopyOwner>,
) -> bool {
    if !removal.uninstall || pool_owned {
        return false;
    }
    match copy_owner {
        Some(CopyOwner::Machine) => removal.per_machine,
        Some(CopyOwner::User(owner)) => !removal.per_machine && removal.invoker == Some(owner),
        None => false,
    }
}
