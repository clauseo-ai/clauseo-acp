//! The admin copy's refresh (ADR 0049, "Admin copy refresh"): `clauseo-sandbox-setup.exe
//! --refresh`, run by the SYSTEM task `\Clauseo\SandboxRefresh`, copies a newer, validly signed
//! setup exe from the per-user install folder over the admin-only copy under Program Files,
//! with no prompt.
//!
//! [`refresh`] holds the order and the refusals, with no Windows call; every effect goes
//! through [`Refresher`], which `windows_refresher.rs` implements and the tests fake. It:
//!
//! 1. refuses unless this process is LOCAL SYSTEM and runs from the admin copy's path;
//! 2. refuses a build whose updater key is the placeholder, before reading anything; then tries
//!    the set-up lock (`setup::lock`'s named mutex, which `--adopt`, the set-up and `--remove`
//!    take) without waiting and holds it to the end of the run, so `--adopt` never hashes or
//!    records the copy while a refresh reads the record or swaps the copy; held, it exits 44;
//! 3. reads the record (`HKLM\SOFTWARE\Clauseo\Sandbox`): `AdminCopyOwner`, `UserInstallDir`
//!    (a plain local path, which must lie inside the owner's profile as `ProfileList` names it,
//!    a temporary profile counting as none; for a `machine` owner, a folder only SYSTEM,
//!    Administrators or `TrustedInstaller` can change, `--adopt --per-machine`'s test) and
//!    `AdminCopyVersion`; nothing else comes from outside: no argument, no environment. Not
//!    `OwnerSid`: the pool's owner, which the in-app `--remove` deletes while the copy stays;
//! 4. clears what an earlier refresh left in staging;
//! 5. reads the exe and its `.sig` from the install folder, which the lawyer controls and may
//!    re-point at any time: every folder of its path is opened relative to its parent's handle
//!    and refused if a reparse point, then the two files relative to the last one, each a regular
//!    file with one link, at most [`EXE_CAP`] and [`SIG_CAP`] bytes (`walk.rs`). Nothing is ever
//!    written or deleted there;
//! 6. copies the exe into the SYSTEM-only staging folder and reads it back from there;
//! 7. verifies the staged bytes' signature (`signature.rs`), never the user folder's file, so
//!    a swap after the check changes nothing; requires the staged image's CPU type (`Machine`)
//!    to be this copy's, so an exe for another CPU never replaces it; requires the staged exe's
//!    `FILEVERSION` (`pe_version.rs`) to be the signed `version`'s core with a zero fourth
//!    part; then refuses a signed version older than the record's `AdminCopyVersion` by semver
//!    precedence (`version.rs`, [`version_not_older`]: a prerelease is older than its release);
//! 8. stops when the staged SHA-256 is the record's `AdminCopySha256`; when it is the hash of
//!    the file at the admin path (an earlier refresh stopped before its record writes), writes
//!    the record and stops; refuses a version equal to the recorded one whose bytes differ;
//! 9. gives the staged file the admin copy's DACL, opens the admin folder for adding a file,
//!    sharing read, write and delete, and holds it until the renames are done, so no one can
//!    hold the folder against them (a failed open stops the run before anything moves); then
//!    sets the running admin copy aside in staging
//!    (a rename, which a running exe allows) and moves the staged file into place, back to back
//!    and write-through, and only then writes the new `AdminCopyVersion` (the signed version,
//!    as signed), then the new `AdminCopySha256`. The set-aside copy is the running exe, which
//!    Windows won't delete while it runs: it and the staging folder are scheduled for deletion
//!    at the next restart, and the next run, which starts from the new copy and so proves it
//!    runs, deletes them sooner;
//! 10. once step 4 has run, every run ends by leaving no staging folder: the staged file is
//!     deleted and the folder removed, or, after a swap, both scheduled as step 9 says. A
//!     staging folder an older refresh left, or one killed midway, is taken back by `--adopt`
//!     and `--remove --uninstall` (`admin_copy::take_back`).
//!
//! The record never names a hash the admin path doesn't hold: until the hash write it holds the
//! old hash. A crash after both renames leaves the new file under the old hash, which the
//! launcher refuses until the next run's step 8 writes its version and hash; the version goes
//! first, so a recorded new hash always has its version. A failed move puts the old copy back.
//! A crash, or the task's 5-minute kill, between the two renames is the one state no run can
//! heal: no exe is at the task's path, so the task can't start. The launcher then finds the
//! copy missing (`AdminCopyMissing`), and the app's words lead the lawyer to the installer's
//! repair, which places a copy again; the old copy is still in staging under its set-aside
//! name, so repair can restore it too. The gap is two write-through renames with nothing
//! between them.
//!
//! Any refusal keeps the old copy, writes one log line (the Event Log on Windows) naming no user
//! path beyond the install folder, and exits with its own code ([`RefreshExit`]).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(test)]
mod fake;
#[cfg(windows)]
pub mod installed;
pub mod key;
#[cfg(test)]
mod key_tests;
pub mod last_run;
pub mod pe_version;
#[cfg(test)]
mod refusal_tests;
mod rules;
pub mod signature;
#[cfg(windows)]
mod source;
mod sources;
#[cfg(windows)]
mod staging;
#[cfg(windows)]
pub mod task;
#[cfg(any(windows, test))]
mod task_folder;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
mod test_signer;
pub mod version;
#[cfg(windows)]
mod walk;
#[cfg(windows)]
mod windows_refresher;
#[cfg(test)]
#[cfg(windows)]
pub(crate) mod windows_tests;

#[cfg(windows)]
pub use installed::{installed_setup_sha256, recorded_admin_copy_sha256};
pub use rules::{
    EXECUTION_LIMIT, REFRESH_ARG, STAGING_SDDL, STOP_ARG, STOP_EXECUTION_LIMIT, SystemTask, TASKS,
    TEARDOWN, Teardown, WATCH_ARG, WATCH_EXECUTION_LIMIT, action_path, hex, inside_profile,
    same_path, sddl_matches, task_sddl,
};
pub use sources::{SourceError, SourceFile, Sources};
#[cfg(windows)]
pub use task::{
    TASK_FOLDER, TaskError, last_refresh_run, register_task, request_admin_refresh,
    request_pool_stop, request_watch, unregister_task,
};
pub use version::version_not_older;
#[cfg(windows)]
pub use windows_refresher::{WindowsRefresher, event_log, run};

use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use signature::{SignatureError, verify};

/// The most bytes the refresh reads of the user folder's exe: 16 MiB.
pub const EXE_CAP: u64 = 16 * 1024 * 1024;
/// The most bytes it reads of the `.sig`: 4 KiB.
pub const SIG_CAP: u64 = 4 * 1024;
/// The updater key built in (`build.rs`): Tauri's base64 of a minisign public key, the
/// placeholder in a build given none.
pub const UPDATER_KEY: &str = env!("CLAUSEO_SANDBOX_UPDATER_KEY");
/// This exe's COFF `Machine`, the only one a staged exe may have: the admin copy is this exe.
pub const OWN_MACHINE: u16 = if cfg!(target_arch = "aarch64") {
    pe_version::MACHINE_ARM64
} else if cfg!(target_arch = "x86") {
    pe_version::MACHINE_X86
} else {
    pe_version::MACHINE_X64
};

/// What a staged exe is checked against: the updater key and this copy's CPU type
/// ([`UPDATER_KEY`] and [`OWN_MACHINE`] in the exe; the tests pass their own).
#[derive(Debug, Clone, Copy)]
pub struct Trust<'a> {
    /// Tauri's base64 of the minisign public key.
    pub key: &'a str,
    /// The COFF `Machine` the staged image must have.
    pub machine: u16,
}

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
    /// 33: the record can't be read, or lacks a usable `AdminCopyOwner`, `UserInstallDir` or
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
    /// 40: the signed version is older than the record's `AdminCopyVersion` (or the same, with
    /// other bytes), or a version or the `FILEVERSION` can't be read.
    Downgrade,
    /// 41: `UserInstallDir` isn't inside the owner's profile, or the owner has none; for a
    /// `machine` owner, it isn't a folder only administrators can change.
    OutsideProfile,
    /// 42: the staged exe is for another CPU type than this copy.
    WrongMachine,
    /// 43: the admin folder couldn't be held for the renames (someone holds it); nothing moved.
    FolderBusy,
    /// 44: the set-up lock is held (an `--adopt`, set-up or removal is running), or can't be
    /// taken; nothing was read or changed.
    LockHeld,
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
            RefreshExit::WrongMachine => 42,
            RefreshExit::FolderBusy => 43,
            RefreshExit::LockHeld => 44,
        }
    }
}

/// The values of the record the refresh reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshRecord {
    /// `AdminCopyOwner`: whose install placed the copy, a SID or `machine`.
    pub copy_owner: Option<String>,
    /// `UserInstallDir`: the per-user install folder, as `--adopt` recorded it.
    pub user_install_dir: Option<String>,
    /// `AdminCopySha256`: the admin copy's SHA-256, 64 lowercase hex digits.
    pub admin_copy_sha256: Option<String>,
    /// `AdminCopyVersion`: the admin copy's file version.
    pub admin_copy_version: Option<String>,
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
    /// Tries the set-up lock without waiting and keeps it until [`Refresher::unlock`]:
    /// `Ok(false)` while another holds it.
    fn lock(&mut self) -> Result<bool, Failed>;
    /// Gives the set-up lock back, if held.
    fn unlock(&mut self);
    /// The record's values; a missing key or value is `None`.
    fn record(&mut self) -> Result<RefreshRecord, Failed>;
    /// `owner`'s profile folder (`ProfileList`'s `ProfileImagePath`); `None` when it has none.
    fn profile_dir(&mut self, owner: &Sid) -> Result<Option<PathBuf>, Failed>;
    /// Whether `dir` is a folder, no link, that only SYSTEM, Administrators or
    /// `TrustedInstaller` can change (`admin_copy::level_safe`), as `--adopt --per-machine`
    /// requires of its install folder.
    fn admin_only_dir(&mut self, dir: &Path) -> Result<bool, Failed>;
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
    /// Opens the admin folder for adding a file, sharing read, write and delete, and keeps the
    /// handle until [`Refresher::release_admin_folder`]: while it is open, no one can open the
    /// folder in a way that fails the renames into it.
    fn hold_admin_folder(&mut self) -> Result<(), Failed>;
    /// Closes the handle [`Refresher::hold_admin_folder`] opened, if any.
    fn release_admin_folder(&mut self);
    /// Renames the admin copy (this running exe) to the set-aside name in staging.
    fn set_aside_admin_copy(&mut self) -> Result<(), Failed>;
    /// Moves the staged file to the admin path.
    fn place_staged(&mut self) -> Result<(), Failed>;
    /// Moves the set-aside copy back to the admin path.
    fn restore_admin_copy(&mut self) -> Result<(), Failed>;
    /// Writes `AdminCopyVersion` to the record.
    fn write_version(&mut self, version: &str) -> Result<(), Failed>;
    /// Writes `AdminCopySha256` (64 lowercase hex digits) to the record.
    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed>;
    /// Deletes the staged file, if any; a failure is only logged by the next run.
    fn discard_staged(&mut self);
    /// Step 10: removes the staging folder, empty once the staged file is gone; after a swap
    /// (`swapped`), when it holds the running set-aside copy, schedules the copy and the folder
    /// for deletion at restart instead. A failure is logged; the next run clears it.
    fn leave_staging(&mut self, swapped: bool);
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// Runs the refresh as the module says, checking the staged exe against `trust`.
pub fn refresh(fs: &mut impl Refresher, trust: &Trust<'_>) -> RefreshExit {
    match run_steps(fs, trust) {
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
fn run_steps(fs: &mut impl Refresher, trust: &Trust<'_>) -> Result<String, Refusal> {
    allowed(fs)?;
    let placeholder = key::key_id(trust.key).is_ok_and(|id| id == key::PLACEHOLDER_KEY_ID);
    if placeholder {
        return Err((
            RefreshExit::PlaceholderKey,
            SignatureError::PlaceholderKey.to_string(),
        ));
    }
    let busy = |why: String| (RefreshExit::LockHeld, why);
    if !fs.lock().map_err(busy)? {
        return Err(busy(
            "a set-up, removal or --adopt holds the set-up lock".to_owned(),
        ));
    }
    let locked = locked_steps(fs, trust);
    fs.unlock();
    locked
}

/// Steps 3 to 9, with the set-up lock held.
fn locked_steps(fs: &mut impl Refresher, trust: &Trust<'_>) -> Result<String, Refusal> {
    let record = fs.record().map_err(|why| (RefreshExit::NoRecord, why))?;
    let checked = checked(fs, record)?;
    if let Err(why) = fs.clear_leftovers() {
        fs.log(true, &format!("an earlier refresh's leftovers stay: {why}"));
    }
    let outcome = staged_steps(fs, trust, &checked);
    let swapped = matches!(outcome, Ok(Replaced::Swapped));
    if !swapped {
        fs.discard_staged();
    }
    fs.leave_staging(swapped);
    outcome.map(|replaced| replaced.line().to_owned())
}

/// Steps 5 to 9, after step 4 cleared staging: whatever they end with, [`locked_steps`] leaves
/// no staging folder behind (step 10).
fn staged_steps(
    fs: &mut impl Refresher,
    trust: &Trust<'_>,
    checked: &Checked,
) -> Result<Replaced, Refusal> {
    let sources = fs.read_sources(&checked.dir).map_err(|(which, error)| {
        let name = match which {
            SourceFile::Folder => "the folder",
            SourceFile::Exe => "clauseo-sandbox-setup.exe",
            SourceFile::Sig => "clauseo-sandbox-setup.exe.sig",
        };
        let line = format!("{name} in the install folder: {error}");
        (RefreshExit::SourceRefused, line)
    })?;
    let staged = fs
        .stage(&sources.exe)
        .map_err(|why| (RefreshExit::StagingFailed, why))?;
    replace(fs, &staged, &sources.sig, checked, trust)
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
        return Err(refuse(
            "the refresh runs only from the admin copy".to_owned(),
        ));
    }
    Ok(())
}

/// Step 3: the record's `AdminCopyOwner`, its `UserInstallDir` (a full local path from a drive
/// letter with no `.`, `..`, stream or trailing-dot part, `args::data_dir_of`'s rules, inside
/// the owner's profile, or administrators-only for `machine`) and its `AdminCopyVersion`.
fn checked(fs: &mut impl Refresher, record: RefreshRecord) -> Result<Checked, Refusal> {
    let refuse = |why: &str| (RefreshExit::NoRecord, why.to_owned());
    let owner = record
        .copy_owner
        .as_deref()
        .and_then(CopyOwner::parse)
        .ok_or_else(|| refuse("the record has no usable AdminCopyOwner"))?;
    let dir = record
        .user_install_dir
        .as_deref()
        .ok_or_else(|| refuse("the record has no UserInstallDir"))?;
    // The refusal quotes the path, so it isn't logged.
    let dir = crate::windows::setup::args::data_dir_of(Some(dir.into()))
        .map_err(|_usage| refuse("the record's UserInstallDir isn't a usable path"))?;
    let version = record
        .admin_copy_version
        .ok_or_else(|| refuse("the record has no AdminCopyVersion"))?;
    let outside = |why: String| (RefreshExit::OutsideProfile, why);
    match owner {
        CopyOwner::User(owner) => {
            let profile = fs
                .profile_dir(&owner)
                .map_err(outside)?
                .ok_or_else(|| outside("the owner has no profile".to_owned()))?;
            if !inside_profile(&dir, &profile) {
                return Err(outside(
                    "the record's UserInstallDir isn't inside the owner's profile".to_owned(),
                ));
            }
        }
        CopyOwner::Machine => {
            if !fs.admin_only_dir(&dir).map_err(outside)? {
                return Err(outside(
                    "the record's UserInstallDir isn't a folder only administrators can change"
                        .to_owned(),
                ));
            }
        }
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
    trust: &Trust<'_>,
) -> Result<Replaced, Refusal> {
    let signed = verify(staged, sig, trust.key).map_err(|error| refused(&error))?;
    let machine = pe_version::machine(staged);
    if machine != Some(trust.machine) {
        let found = machine.map_or_else(|| "none".to_owned(), |m| format!("{m:#06x}"));
        let why = format!(
            "the staged exe is for machine {found}, not this copy's {:#06x}",
            trust.machine
        );
        return Err((RefreshExit::WrongMachine, why));
    }
    let file = pe_version::file_version(staged).ok_or_else(|| {
        let why = "the staged exe carries no readable FILEVERSION".to_owned();
        (RefreshExit::Downgrade, why)
    })?;
    if !version::matches_file_version(&signed.version, file) {
        return Err(refused(&SignatureError::VersionMismatch {
            signed: signed.version,
            staged: pe_version::dotted(file),
        }));
    }
    let order = version::compare(&checked.version, &signed.version);
    if !order.is_some_and(Ordering::is_ge) {
        let why = format!(
            "the signed version {} is older than the recorded {}, or unreadable",
            signed.version, checked.version
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
        write_record(fs, &signed.version, &hex)?;
        return Ok(Replaced::RecordWritten);
    }
    if order == Some(Ordering::Equal) {
        let why = format!(
            "the signed version {} is the recorded one, but its bytes differ from the copy in place",
            signed.version
        );
        return Err((RefreshExit::Downgrade, why));
    }
    fs.secure_staged()
        .map_err(|why| (RefreshExit::StagingFailed, why))?;
    fs.hold_admin_folder()
        .map_err(|why| (RefreshExit::FolderBusy, why))?;
    let swapped = swap(fs);
    fs.release_admin_folder();
    swapped?;
    write_record(fs, &signed.version, &hex)?;
    Ok(Replaced::Swapped)
}

/// The two renames, with the admin folder held; a failed move puts the old copy back.
fn swap(fs: &mut impl Refresher) -> Result<(), Refusal> {
    // The gap: from here to the move, no exe is at the task's path. Nothing runs in between.
    fs.set_aside_admin_copy()
        .map_err(|why| (RefreshExit::ReplaceFailed, why))?;
    if let Err(why) = fs.place_staged() {
        let restored = match fs.restore_admin_copy() {
            Ok(()) => "the old copy is back in place".to_owned(),
            Err(also) => format!("the old copy couldn't be put back either ({also})"),
        };
        return Err((RefreshExit::ReplaceFailed, format!("{why}; {restored}")));
    }
    Ok(())
}

/// A refused signature's exit (35, or 36 for the placeholder key) and log line.
fn refused(error: &SignatureError) -> Refusal {
    let exit = match error {
        SignatureError::PlaceholderKey => RefreshExit::PlaceholderKey,
        SignatureError::BadKey(_)
        | SignatureError::Malformed(_)
        | SignatureError::WrongKey
        | SignatureError::NotPrehashed
        | SignatureError::Invalid
        | SignatureError::WrongFile
        | SignatureError::BadComment(_)
        | SignatureError::NoVersion
        | SignatureError::VersionMismatch { .. } => RefreshExit::SignatureRefused,
    };
    (exit, error.to_string())
}

/// The record's version, then its hash: a recorded new hash always has its version.
fn write_record(fs: &mut impl Refresher, version: &str, hex: &str) -> Result<(), Refusal> {
    let unwritten = |why| (RefreshExit::RecordNotWritten, why);
    fs.write_version(version).map_err(unwritten)?;
    fs.write_sha256(hex).map_err(unwritten)
}

#[cfg(test)]
mod tests;
