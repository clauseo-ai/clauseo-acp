//! The admin copy's refresh (ADR 0049, "Admin copy refresh"): `clauseo-sandbox-setup.exe
//! --refresh`, run by the SYSTEM task `\Clauseo\SandboxRefresh`, copies a newer, validly signed
//! setup exe from the per-user install folder over the admin-only copy under Program Files,
//! with no prompt.
//!
//! [`refresh`] holds the order and the refusals, with no Windows call; every effect goes
//! through [`Refresher`], which `windows_refresher.rs` implements and the tests fake. It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path;
//! 2. refuses a build whose updater key is the placeholder, before reading anything;
//! 3. reads the record (`HKLM\SOFTWARE\Clauseo\Sandbox`): `OwnerSid`, `UserInstallDir` (a plain
//!    local path, which must lie inside the owner's profile as `ProfileList` names it) and
//!    `AdminCopyVersion`; nothing else comes from outside: no argument, no environment;
//! 4. clears what an earlier refresh left in staging;
//! 5. reads the exe and its `.sig` from the install folder, which the lawyer controls and may
//!    re-point at any time: every folder of its path is opened relative to its parent's handle
//!    and refused if a reparse point, then the two files relative to the last one, each a regular
//!    file with one link, at most [`EXE_CAP`] and [`SIG_CAP`] bytes (`walk.rs`). Nothing is ever
//!    written or deleted there;
//! 6. copies the exe into the SYSTEM-only staging folder and reads it back from there;
//! 7. verifies the staged bytes' signature (`signature.rs`), never the user folder's file, so
//!    a swap after the check changes nothing; then refuses a staged exe whose `FILEVERSION`
//!    (`pe_version.rs`) is older than the record's `AdminCopyVersion` ([`version_not_older`]);
//! 8. stops when the staged SHA-256 is the record's `AdminCopySha256`; when it is the hash of
//!    the file at the admin path (an earlier refresh stopped before its record writes), writes
//!    the record and stops;
//! 9. gives the staged file the admin copy's DACL, sets the running admin copy aside (a rename,
//!    which a running exe allows), moves the staged file into place, and only then writes the
//!    new `AdminCopyVersion`, then the new `AdminCopySha256`; the set-aside copy is deleted at
//!    the next reboot or the next run.
//!
//! A crash never leaves the record naming a file that isn't at the admin path. Until the hash
//! write, the record holds the old hash: a crash between the two renames leaves no file at the
//! admin path, which the launcher treats as a missing copy; a crash after them leaves the new
//! file under the old hash, which the launcher refuses until the next run's step 8 writes its
//! version and hash. The version goes first, so a recorded new hash always has its version. A
//! failed move puts the old copy back.
//!
//! Any refusal keeps the old copy, writes one log line (the Event Log on Windows) naming no user
//! path beyond the install folder, and exits with its own code ([`RefreshExit`]).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(test)]
mod fake;
pub mod key;
#[cfg(test)]
mod key_tests;
pub mod pe_version;
#[cfg(test)]
mod refusal_tests;
mod rules;
pub mod signature;
#[cfg(windows)]
mod source;
#[cfg(windows)]
mod staging;
#[cfg(windows)]
pub mod task;
#[cfg(test)]
mod test_signer;
#[cfg(windows)]
mod walk;
#[cfg(windows)]
mod windows_refresher;

pub use pe_version::version_not_older;
pub use rules::{
    ADMIN_COPY_SDDL, EXECUTION_LIMIT, REFRESH_ARG, STAGING_SDDL, hex, inside_profile, same_path,
    sddl_matches, task_sddl,
};
#[cfg(windows)]
pub use task::{
    TASK_FOLDER, TASK_NAME, TaskError, register_refresh_task, remove_staging,
    request_admin_refresh, unregister_refresh_task,
};
#[cfg(windows)]
pub use windows_refresher::{WindowsRefresher, run};

use crate::windows::sids::Sid;
use signature::{SignatureError, verify};

/// The most bytes the refresh reads of the user folder's exe: 16 MiB.
pub const EXE_CAP: u64 = 16 * 1024 * 1024;
/// The most bytes it reads of the `.sig`: 4 KiB.
pub const SIG_CAP: u64 = 4 * 1024;
/// The updater key built in (`build.rs`): Tauri's base64 of a minisign public key, the
/// placeholder in a build given none.
pub const UPDATER_KEY: &str = env!("CLAUSEO_SANDBOX_UPDATER_KEY");

/// The refresh's exit codes. They start at 32, clear of [`SetupExit`]'s, so the two never share
/// a number whatever either appends; codes are appended from here on, never renumbered.
///
/// [`SetupExit`]: crate::windows::setup::SetupExit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshExit {
    /// 0: the admin copy is current (refreshed, already current, or its record written).
    Done,
    /// 32: not LOCAL SYSTEM, or not run from the admin copy's path.
    NotAllowed,
    /// 33: the record can't be read, or lacks a usable `OwnerSid`, `UserInstallDir` or
    /// `AdminCopyVersion`.
    NoRecord,
    /// 34: the user folder's exe or `.sig` was refused: missing, a reparse point on the way or
    /// at the file, not a regular file, more than one link, over its cap, or unreadable.
    SourceRefused,
    /// 35: the staged exe's signature was refused (`SignatureError`).
    SignatureRefused,
    /// 36: this build's updater key is the placeholder; nothing was read.
    PlaceholderKey,
    /// 37: the staging folder or the staged copy couldn't be made, read back or secured.
    StagingFailed,
    /// 38: the admin copy couldn't be replaced; the old copy is in place.
    ReplaceFailed,
    /// 39: the new copy is in place, but the record's version or hash couldn't be written; the
    /// launcher refuses the copy until the next run writes them.
    RecordNotWritten,
    /// 40: the staged exe is older than the record's `AdminCopyVersion`, or carries no readable
    /// `FILEVERSION`.
    Downgrade,
    /// 41: `UserInstallDir` isn't inside the owner's profile, or the owner has none.
    OutsideProfile,
}

impl RefreshExit {
    /// The process exit code.
    pub fn as_code(self) -> u8 {
        match self {
            RefreshExit::Done => 0,
            RefreshExit::NotAllowed => 32,
            RefreshExit::NoRecord => 33,
            RefreshExit::SourceRefused => 34,
            RefreshExit::SignatureRefused => 35,
            RefreshExit::PlaceholderKey => 36,
            RefreshExit::StagingFailed => 37,
            RefreshExit::ReplaceFailed => 38,
            RefreshExit::RecordNotWritten => 39,
            RefreshExit::Downgrade => 40,
            RefreshExit::OutsideProfile => 41,
        }
    }
}

/// The values of the record the refresh reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshRecord {
    /// `OwnerSid`: the Windows user the record belongs to.
    pub owner_sid: Option<String>,
    /// `UserInstallDir`: the per-user install folder, as `--adopt` recorded it.
    pub user_install_dir: Option<String>,
    /// `AdminCopySha256`: the admin copy's SHA-256, 64 lowercase hex digits.
    pub admin_copy_sha256: Option<String>,
    /// `AdminCopyVersion`: the admin copy's file version.
    pub admin_copy_version: Option<String>,
}

/// Why the user folder's exe or `.sig` was refused. Names no path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Not there.
    #[error("the file isn't there")]
    Missing,
    /// A reparse point (a link, a junction, a cloud placeholder, ...) at the file or on the way
    /// to it; tag 0 when the open met it on the way.
    #[error("a reparse point (tag {0:#x}) is in the way")]
    ReparsePoint(u32),
    /// A folder, a device, or anything but a regular file.
    #[error("the file isn't a regular file")]
    NotRegular,
    /// More than one hard link.
    #[error("the file has {0} hard links")]
    Linked(u32),
    /// Over its cap.
    #[error("the file is over {0} bytes")]
    TooLarge(u64),
    /// A Windows call failed.
    #[error("{call} failed ({code})")]
    Os {
        /// The call.
        call: &'static str,
        /// Its Win32 error code.
        code: u32,
    },
}

/// Which file of the user folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFile {
    /// The folder chain itself, before either file.
    Folder,
    /// `clauseo-sandbox-setup.exe`.
    Exe,
    /// `clauseo-sandbox-setup.exe.sig`.
    Sig,
}

/// The two files read from the install folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    /// The exe's bytes.
    pub exe: Vec<u8>,
    /// The `.sig`'s bytes.
    pub sig: Vec<u8>,
}

/// A failed effect: what failed, as a line for the log. Names no user path.
pub type Failed = String;

/// Every effect of the refresh. The real one holds the fixed paths (the admin copy, the
/// staging folder) itself, so no method takes a path the caller chose; `read_sources` takes the
/// folder the record names, and only reads it.
pub trait Refresher {
    /// Whether this process's token user is LOCAL SYSTEM.
    fn is_system(&mut self) -> Result<bool, Failed>;
    /// This process's exe.
    fn current_exe(&mut self) -> Result<PathBuf, Failed>;
    /// The admin copy's path: `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe`.
    fn admin_copy(&mut self) -> Result<PathBuf, Failed>;
    /// The record's values; a missing key or value is `None`.
    fn record(&mut self) -> Result<RefreshRecord, Failed>;
    /// `owner`'s profile folder (`ProfileList`'s `ProfileImagePath`); `None` when it has none.
    fn profile_dir(&mut self, owner: &Sid) -> Result<Option<PathBuf>, Failed>;
    /// Deletes what an earlier run left in staging (the set-aside copy, a staged file).
    fn clear_leftovers(&mut self) -> Result<(), Failed>;
    /// The exe and `.sig` of the folder `dir`, opened as step 5 says; which one failed, and why.
    fn read_sources(&mut self, dir: &Path) -> Result<Sources, (SourceFile, SourceError)>;
    /// Makes the staging folder (SYSTEM only, read back), writes `exe` there as the staged
    /// file, and returns the staged file's bytes as read back from it.
    fn stage(&mut self, exe: &[u8]) -> Result<Vec<u8>, Failed>;
    /// The SHA-256 of the file now at the admin path; `None` when there is none.
    fn admin_copy_sha256(&mut self) -> Result<Option<[u8; 32]>, Failed>;
    /// Gives the staged file the admin copy's owner and protected DACL.
    fn secure_staged(&mut self) -> Result<(), Failed>;
    /// Renames the admin copy (this running exe) to the set-aside name in staging.
    fn set_aside_admin_copy(&mut self) -> Result<(), Failed>;
    /// Moves the staged file to the admin path.
    fn place_staged(&mut self) -> Result<(), Failed>;
    /// Moves the set-aside copy back to the admin path.
    fn restore_admin_copy(&mut self) -> Result<(), Failed>;
    /// Schedules the set-aside copy's deletion at the next reboot.
    fn delete_set_aside_later(&mut self) -> Result<(), Failed>;
    /// Writes `AdminCopyVersion` to the record.
    fn write_version(&mut self, version: &str) -> Result<(), Failed>;
    /// Writes `AdminCopySha256` (64 lowercase hex digits) to the record.
    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed>;
    /// Deletes the staged file, if any; a failure is only logged by the next run.
    fn discard_staged(&mut self);
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// Runs the refresh as the module says, with the key `key` ([`UPDATER_KEY`] in the exe; the
/// tests pass their own).
pub fn refresh(fs: &mut impl Refresher, key: &str) -> RefreshExit {
    match run_steps(fs, key) {
        Ok(line) => {
            fs.log(false, &line);
            RefreshExit::Done
        }
        Err((exit, line)) => {
            fs.log(true, &format!("the sandbox's admin copy was kept: {line}"));
            exit
        }
    }
}

/// A refusal: its exit and its log line.
type Refusal = (RefreshExit, String);

/// What the record gives a run, checked.
struct Checked {
    dir: PathBuf,
    sha256: Option<String>,
    version: String,
}

/// The steps; `Ok` carries the log line of a run that left the admin copy current.
fn run_steps(fs: &mut impl Refresher, key: &str) -> Result<String, Refusal> {
    allowed(fs)?;
    let placeholder = key::key_id(key).is_ok_and(|id| id == key::PLACEHOLDER_KEY_ID);
    if placeholder {
        return Err((
            RefreshExit::PlaceholderKey,
            SignatureError::PlaceholderKey.to_string(),
        ));
    }
    let record = fs.record().map_err(|why| (RefreshExit::NoRecord, why))?;
    let checked = checked(fs, record)?;
    if let Err(why) = fs.clear_leftovers() {
        fs.log(true, &format!("an earlier refresh's leftovers stay: {why}"));
    }
    let sources = fs.read_sources(&checked.dir).map_err(|(which, error)| {
        let name = match which {
            SourceFile::Folder => "the folder",
            SourceFile::Exe => "clauseo-sandbox-setup.exe",
            SourceFile::Sig => "clauseo-sandbox-setup.exe.sig",
        };
        let line = format!("{name} in {}: {error}", checked.dir.display());
        (RefreshExit::SourceRefused, line)
    })?;
    let staged = fs
        .stage(&sources.exe)
        .map_err(|why| (RefreshExit::StagingFailed, why))?;
    let outcome = replace(fs, &staged, &sources.sig, &checked, key);
    if !matches!(outcome, Ok(Replaced::Swapped)) {
        fs.discard_staged();
    }
    outcome.map(|replaced| replaced.line().to_owned())
}

/// What [`replace`] did.
enum Replaced {
    /// The record already named the staged exe.
    Current,
    /// The admin path already held it; the record now names it.
    RecordWritten,
    /// The staged exe is the admin copy now.
    Swapped,
}

impl Replaced {
    /// Its log line.
    fn line(&self) -> &'static str {
        match self {
            Replaced::Current => "the sandbox's admin copy is current",
            Replaced::RecordWritten => {
                "the sandbox's admin copy was current; its record is written"
            }
            Replaced::Swapped => "the sandbox's admin copy was refreshed",
        }
    }
}

/// Step 1: LOCAL SYSTEM, from the admin copy's path.
fn allowed(fs: &mut impl Refresher) -> Result<(), Refusal> {
    let refuse = |why: String| (RefreshExit::NotAllowed, why);
    if !fs.is_system().map_err(refuse)? {
        return Err(refuse("the refresh runs only as LOCAL SYSTEM".to_owned()));
    }
    let exe = fs.current_exe().map_err(refuse)?;
    let admin = fs.admin_copy().map_err(refuse)?;
    if !same_path(&exe, &admin) {
        return Err(refuse(format!(
            "the refresh runs only from {}",
            admin.display()
        )));
    }
    Ok(())
}

/// Step 3: the record's owner, its `UserInstallDir` (a full local path from a drive letter
/// with no `.`, `..`, stream or trailing-dot part, `args::data_dir_of`'s rules, inside the
/// owner's profile) and its `AdminCopyVersion`.
fn checked(fs: &mut impl Refresher, record: RefreshRecord) -> Result<Checked, Refusal> {
    let refuse = |why: &str| (RefreshExit::NoRecord, why.to_owned());
    let owner = record
        .owner_sid
        .as_deref()
        .and_then(|text| Sid::parse(text).ok())
        .ok_or_else(|| refuse("the record has no usable OwnerSid"))?;
    let dir = record
        .user_install_dir
        .as_deref()
        .ok_or_else(|| refuse("the record has no UserInstallDir"))?;
    let dir = crate::windows::setup::args::data_dir_of(Some(dir.into())).map_err(|usage| {
        (
            RefreshExit::NoRecord,
            format!("the record's UserInstallDir: {}", usage.0),
        )
    })?;
    let version = record
        .admin_copy_version
        .ok_or_else(|| refuse("the record has no AdminCopyVersion"))?;
    let outside = |why: String| (RefreshExit::OutsideProfile, why);
    let profile = fs
        .profile_dir(&owner)
        .map_err(outside)?
        .ok_or_else(|| outside(format!("{owner} has no profile")))?;
    if !inside_profile(&dir, &profile) {
        return Err(outside(format!(
            "{} isn't inside {owner}'s profile",
            dir.display()
        )));
    }
    Ok(Checked {
        dir,
        sha256: record.admin_copy_sha256,
        version,
    })
}

/// Steps 7 to 9 on the staged bytes.
fn replace(
    fs: &mut impl Refresher,
    staged: &[u8],
    sig: &[u8],
    checked: &Checked,
    key: &str,
) -> Result<Replaced, Refusal> {
    verify(staged, sig, key).map_err(|error| {
        let exit = match error {
            SignatureError::PlaceholderKey => RefreshExit::PlaceholderKey,
            SignatureError::BadKey(_)
            | SignatureError::Malformed(_)
            | SignatureError::WrongKey
            | SignatureError::NotPrehashed
            | SignatureError::Invalid
            | SignatureError::WrongFile => RefreshExit::SignatureRefused,
        };
        (exit, error.to_string())
    })?;
    let version = pe_version::file_version(staged).map(pe_version::dotted);
    let version = version.ok_or_else(|| {
        let why = "the staged exe carries no readable FILEVERSION".to_owned();
        (RefreshExit::Downgrade, why)
    })?;
    if !version_not_older(&checked.version, &version) {
        let why = format!(
            "the staged exe's version {version} is older than the recorded {}",
            checked.version
        );
        return Err((RefreshExit::Downgrade, why));
    }
    let sha: [u8; 32] = Sha256::digest(staged).into();
    let hex = hex(&sha);
    if checked.sha256.as_deref() == Some(hex.as_str()) {
        return Ok(Replaced::Current);
    }
    let at_admin = fs
        .admin_copy_sha256()
        .map_err(|why| (RefreshExit::ReplaceFailed, why))?;
    if at_admin == Some(sha) {
        write_record(fs, &version, &hex)?;
        return Ok(Replaced::RecordWritten);
    }
    fs.secure_staged()
        .map_err(|why| (RefreshExit::StagingFailed, why))?;
    fs.set_aside_admin_copy()
        .map_err(|why| (RefreshExit::ReplaceFailed, why))?;
    if let Err(why) = fs.place_staged() {
        let restored = match fs.restore_admin_copy() {
            Ok(()) => "the old copy is back in place".to_owned(),
            Err(also) => format!("the old copy couldn't be put back either ({also})"),
        };
        return Err((RefreshExit::ReplaceFailed, format!("{why}; {restored}")));
    }
    if let Err(why) = fs.delete_set_aside_later() {
        let line = format!("the old copy stays in staging until the next run: {why}");
        fs.log(true, &line);
    }
    write_record(fs, &version, &hex)?;
    Ok(Replaced::Swapped)
}

/// The record's version, then its hash: a recorded new hash always has its version.
fn write_record(fs: &mut impl Refresher, version: &str, hex: &str) -> Result<(), Refusal> {
    let unwritten = |why| (RefreshExit::RecordNotWritten, why);
    fs.write_version(version).map_err(unwritten)?;
    fs.write_sha256(hex).map_err(unwritten)
}

#[cfg(test)]
mod tests;
