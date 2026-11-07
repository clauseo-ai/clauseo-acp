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
//! 2. each of `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy is not a reparse point, is
//!    owned by SYSTEM, Administrators or `TrustedInstaller`, and no allow entry that applies to
//!    it gives any other account write, append, delete (of it or of a child), `WRITE_DAC`,
//!    `WRITE_OWNER` or a generic right that holds one of those ([`level_safe`]): else
//!    [`Blocked::AdminCopyUnsafe`]. Deny entries don't matter: they only take rights away;
//!    The admin folder holds nothing but the copy, its signature and the staging folder
//!    ([`expected_entry`]): a DLL planted beside the copy is refused too;
//! 3. the copy's SHA-256 is the recorded one: else [`Blocked::AdminCopyChanged`].
//!
//! The folder check comes before the hash: a copy in a folder others can change proves nothing
//! by its hash, and naming the folder tells IT what to fix. `%ProgramFiles%` comes from
//! `FOLDERID_ProgramFiles`, never from the environment. Every check reads through
//! [`AdminCopyFs`], so the rules are tested on every OS; `os.rs` is the Windows one.

use std::io;
use std::path::{Path, PathBuf};

use super::acl_bytes::{self, Acl, Descriptor};
use super::setup::record_values::CopyOwner;
use super::sids::Sid;
use crate::{Blocked, SetupState};

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(windows)]
pub(crate) mod os;
#[cfg(test)]
mod tests;

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
    /// The record's `AdminCopySha256`, when it holds one.
    fn recorded_sha256(&self) -> io::Result<Option<String>>;
    /// The names of what is in the folder `dir`.
    fn entries(&self, dir: &Path) -> io::Result<Vec<String>>;
    /// Whether the record names another Windows user than this process's as the pool's owner
    /// (`OwnerSid`) or as the admin copy's (`AdminCopyOwner`, which an in-app removal keeps).
    fn another_owner(&self) -> io::Result<bool>;
}

/// What a tier-B launch (`check_launch`) and every elevated start (`UserSetup`) check first:
/// a pool owned by another Windows user is [`Blocked::OtherAccount`] (D5's one owner), then
/// [`check`]. A record that can't be read is left to the checks after it: the set-up itself
/// enforces the owner.
pub fn check_launchable(fs: &dyn AdminCopyFs) -> Result<PathBuf, Blocked> {
    match fs.another_owner() {
        Ok(true) => {
            tracing::warn!("tier B is set up for another account on this computer");
            return Err(Blocked::OtherAccount);
        }
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "the record's owner can't be read"),
    }
    check(fs)
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
    let program_files = fs.program_files().map_err(|error| {
        tracing::warn!(%error, "Windows gave no Program Files folder");
        Blocked::AdminCopyMissing
    })?;
    let exe = admin_exe(&program_files);
    if !matches!(fs.found(&exe), Ok(Found::File)) {
        tracing::warn!("the set-up program's admin copy isn't there as a regular file");
        return Err(Blocked::AdminCopyMissing);
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
    match fs.sha256(&exe) {
        Ok(actual) if actual == recorded => Ok(exe),
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

/// Rule 2 over `%ProgramFiles%`, `Clauseo`, `Sandbox` and the copy: each a folder (the copy a
/// file) that is no link, with a [`level_safe`] owner and DACL.
fn check_chain(fs: &dyn AdminCopyFs, program_files: &Path, exe: &Path) -> Result<(), Blocked> {
    let mut levels = vec![(program_files.to_path_buf(), Found::Folder)];
    let mut at = program_files.to_path_buf();
    for part in ADMIN_DIR {
        at = at.join(part);
        levels.push((at.clone(), Found::Folder));
    }
    levels.push((exe.to_path_buf(), Found::File));
    for (depth, (path, want)) in levels.iter().enumerate() {
        let found = fs.found(path);
        let secured = found.as_ref().ok() == Some(want)
            && fs.security(path).is_ok_and(|bytes| level_safe(&bytes));
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

/// Whether the self-relative descriptor `bytes` keeps its object to [`TRUSTED`] accounts: an
/// owner among them, a DACL (none would allow everyone everything), and no allow entry that
/// applies to the object itself (inherit-only ones don't) giving any [`CHANGE_RIGHTS`] to
/// another SID. An allow entry whose SID can't be read (object and compound entries) counts as
/// another SID's. Deny, audit and label entries are skipped.
pub fn level_safe(bytes: &[u8]) -> bool {
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
        if mask & CHANGE_RIGHTS == 0 {
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
/// `Blocked::OtherAccount`.
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
            Blocked::AdminCopyMissing | Blocked::AdminCopyChanged | Blocked::AdminCopyUnsafe
        )
    );
    let state = if stale { marker } else { combined };
    if state == SetupState::Needed && fs.another_owner().unwrap_or(false) {
        return SetupState::Blocked(Blocked::OtherAccount);
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

/// Whether `candidate` is no older than `recorded`, both versions as `tauri.conf.json` writes
/// them (`1.2.3`, `1.2.3-beta.1`; build metadata after `+` ignored): `--adopt` and the refresh
/// task (PR 3) never replace a copy with an older one. The numeric parts compare first, up to
/// four (a missing one is 0); with equal parts a release beats a pre-release, and two
/// pre-releases compare part by part as semver does (numbers below words). A version that
/// doesn't parse is never "not older": the safe side.
pub fn version_not_older(recorded: &str, candidate: &str) -> bool {
    match (Version::parse(recorded), Version::parse(candidate)) {
        (Some(recorded), Some(candidate)) => candidate >= recorded,
        _ => false,
    }
}

/// A version as [`version_not_older`] compares it.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    core: [u64; 4],
    /// `true` for a release, so it sorts above any pre-release of the same core.
    release: bool,
    pre: Vec<PreId>,
}

/// One dot-separated pre-release identifier; numbers sort below words.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    Number(u64),
    Word(String),
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let text = text.split('+').next().unwrap_or_default();
        let (core_text, pre_text) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let parts: Vec<u64> = core_text
            .split('.')
            .map(|part| {
                part.bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| part.parse().ok())
                    .flatten()
            })
            .collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }
        let mut core = [0; 4];
        for (slot, part) in core.iter_mut().zip(parts) {
            *slot = part;
        }
        let pre = match pre_text {
            Some(pre) => pre
                .split('.')
                .map(|id| match id.parse() {
                    _ if id.is_empty() => None,
                    Ok(n) if id.bytes().all(|b| b.is_ascii_digit()) => Some(PreId::Number(n)),
                    _ => Some(PreId::Word(id.to_owned())),
                })
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        Some(Version {
            core,
            release: pre_text.is_none(),
            pre,
        })
    }
}
