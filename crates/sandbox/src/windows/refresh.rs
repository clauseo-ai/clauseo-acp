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
//! 3. reads `UserInstallDir` from the record (`HKLM\SOFTWARE\Clauseo\Sandbox`), a plain local
//!    path, and nothing else from outside: no argument, no environment;
//! 4. clears a set-aside copy an earlier refresh left;
//! 5. reads the exe and its `.sig` from that folder without following a reparse point: a
//!    regular file with one link, at most [`EXE_CAP`] and [`SIG_CAP`] bytes;
//! 6. copies the exe into the SYSTEM-only staging folder and reads it back from there;
//! 7. verifies the staged bytes' signature (`signature.rs`), never the user folder's file, so
//!    a swap after the check changes nothing;
//! 8. stops when the staged SHA-256 is the record's `AdminCopySha256`; when it is the hash of
//!    the file at the admin path (an earlier refresh stopped before its record write), writes
//!    the record and stops;
//! 9. gives the staged file the admin copy's DACL, sets the running admin copy aside (a rename,
//!    which a running exe allows), moves the staged file into place, and only then writes the
//!    new hash; the set-aside copy is deleted at the next reboot or the next run.
//!
//! A crash never leaves the record naming a file that isn't at the admin path. Before the
//! record write, the record still holds the old hash: a crash between the two renames leaves no
//! file at the admin path, which the launcher treats as a missing copy; a crash after them
//! leaves the new file under the old hash, which the launcher refuses until the next run's step
//! 8 writes its hash. A failed move puts the old copy back.
//!
//! Any refusal keeps the old copy, writes one log line (the Event Log on Windows) naming no user
//! path beyond the install folder, and exits with its own code ([`RefreshExit`]).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub mod key;
#[cfg(test)]
mod key_tests;
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
mod windows_refresher;

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
/// This exe's version (`crates/desktop/tauri.conf.json`'s `version`), the lowest a refresh
/// takes.
pub const OWN_VERSION: &str = env!("CLAUSEO_SANDBOX_VERSION");

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
    /// 33: the record has no usable `UserInstallDir`, or can't be read.
    NoRecord,
    /// 34: the user folder's exe or `.sig` was refused: missing, a reparse point, not a regular
    /// file, more than one link, over its cap, or unreadable.
    SourceRefused,
    /// 35: the staged exe's signature was refused (`SignatureError`).
    SignatureRefused,
    /// 36: this build's updater key is the placeholder; nothing was read.
    PlaceholderKey,
    /// 37: the staging folder or the staged copy couldn't be made, read back or secured.
    StagingFailed,
    /// 38: the admin copy couldn't be replaced; the old copy is in place.
    ReplaceFailed,
    /// 39: the new copy is in place, but the record's hash couldn't be written; the launcher
    /// refuses the copy until the next run writes it.
    RecordNotWritten,
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
        }
    }
}

/// The two values of the record the refresh reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshRecord {
    /// `UserInstallDir`: the per-user install folder, as `--adopt` recorded it.
    pub user_install_dir: Option<String>,
    /// `AdminCopySha256`: the admin copy's SHA-256, 64 lowercase hex digits.
    pub admin_copy_sha256: Option<String>,
}

/// Why the user folder's exe or `.sig` was refused. Names no path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Not there.
    #[error("the file isn't there")]
    Missing,
    /// A reparse point (a link, a junction, a cloud placeholder, ...).
    #[error("the file is a reparse point (tag {0:#x})")]
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
    /// `clauseo-sandbox-setup.exe`.
    Exe,
    /// `clauseo-sandbox-setup.exe.sig`.
    Sig,
}

/// A failed effect: what failed, as a line for the log. Names no user path.
pub type Failed = String;

/// Every effect of the refresh. The real one holds the fixed paths (the admin copy, the
/// staging folder) itself, so no method takes a path the caller chose; `read_source` takes the
/// folder the record names.
pub trait Refresher {
    /// Whether this process's token user is LOCAL SYSTEM.
    fn is_system(&mut self) -> Result<bool, Failed>;
    /// This process's exe.
    fn current_exe(&mut self) -> Result<PathBuf, Failed>;
    /// The admin copy's path: `<FOLDERID_ProgramFiles>\Clauseo\Sandbox\clauseo-sandbox-setup.exe`.
    fn admin_copy(&mut self) -> Result<PathBuf, Failed>;
    /// The record's two values; a missing key or value is `None`.
    fn record(&mut self) -> Result<RefreshRecord, Failed>;
    /// Deletes what an earlier run left in staging (the set-aside copy, a staged file).
    fn clear_leftovers(&mut self) -> Result<(), Failed>;
    /// `which` of the folder `dir`, opened without following a reparse point, capped.
    fn read_source(&mut self, dir: &Path, which: SourceFile) -> Result<Vec<u8>, SourceError>;
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
    /// Writes `AdminCopySha256` (64 lowercase hex digits) to the record.
    fn write_sha256(&mut self, hex: &str) -> Result<(), Failed>;
    /// Deletes the staged file, if any; a failure is only logged by the next run.
    fn discard_staged(&mut self);
    /// Writes one line to the log: `error` for a refusal or a failure.
    fn log(&mut self, error: bool, line: &str);
}

/// Runs the refresh as the module says, with the key `key` and this exe's version `own`
/// ([`UPDATER_KEY`] and [`OWN_VERSION`] in the exe; the tests pass their own).
pub fn refresh(fs: &mut impl Refresher, key: &str, own: &str) -> RefreshExit {
    match run_steps(fs, key, own) {
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

/// The steps; `Ok` carries the log line of a run that left the admin copy current.
fn run_steps(fs: &mut impl Refresher, key: &str, own: &str) -> Result<String, Refusal> {
    allowed(fs)?;
    let placeholder = key::key_id(key).is_ok_and(|id| id == key::PLACEHOLDER_KEY_ID);
    if placeholder {
        return Err((
            RefreshExit::PlaceholderKey,
            SignatureError::PlaceholderKey.to_string(),
        ));
    }
    let record = fs.record().map_err(|why| (RefreshExit::NoRecord, why))?;
    let dir = install_dir(&record)?;
    if let Err(why) = fs.clear_leftovers() {
        fs.log(true, &format!("an earlier refresh's leftovers stay: {why}"));
    }
    let exe = source(fs, &dir, SourceFile::Exe)?;
    let sig = source(fs, &dir, SourceFile::Sig)?;
    let staged = fs
        .stage(&exe)
        .map_err(|why| (RefreshExit::StagingFailed, why))?;
    let outcome = replace(fs, &staged, &sig, &record, key, own);
    if !matches!(outcome, Ok(Replaced::Swapped)) {
        fs.discard_staged();
    }
    outcome.map(|replaced| replaced.line().to_owned())
}

/// Step 5 for one file.
fn source(fs: &mut impl Refresher, dir: &Path, which: SourceFile) -> Result<Vec<u8>, Refusal> {
    fs.read_source(dir, which).map_err(|error| {
        let name = match which {
            SourceFile::Exe => "clauseo-sandbox-setup.exe",
            SourceFile::Sig => "clauseo-sandbox-setup.exe.sig",
        };
        let line = format!("{name} in {}: {error}", dir.display());
        (RefreshExit::SourceRefused, line)
    })
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
            Replaced::RecordWritten => "the sandbox's admin copy was current; its hash is recorded",
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

/// Step 3: the record's `UserInstallDir`, a full local path from a drive letter with no `.`,
/// `..`, stream or trailing-dot part (`args::data_dir_of`'s rules).
fn install_dir(record: &RefreshRecord) -> Result<PathBuf, Refusal> {
    let refuse = |why: &str| (RefreshExit::NoRecord, why.to_owned());
    let dir = record
        .user_install_dir
        .as_deref()
        .ok_or_else(|| refuse("the record has no UserInstallDir"))?;
    crate::windows::setup::args::data_dir_of(Some(dir.into())).map_err(|usage| {
        (
            RefreshExit::NoRecord,
            format!("the record's UserInstallDir: {}", usage.0),
        )
    })
}

/// Steps 7 to 9 on the staged bytes.
fn replace(
    fs: &mut impl Refresher,
    staged: &[u8],
    sig: &[u8],
    record: &RefreshRecord,
    key: &str,
    own: &str,
) -> Result<Replaced, Refusal> {
    verify(staged, sig, key, own).map_err(|error| {
        let exit = match error {
            SignatureError::PlaceholderKey => RefreshExit::PlaceholderKey,
            SignatureError::BadKey(_)
            | SignatureError::Malformed(_)
            | SignatureError::WrongKey
            | SignatureError::NotPrehashed
            | SignatureError::Invalid
            | SignatureError::WrongFile
            | SignatureError::NoVersion
            | SignatureError::Downgrade { .. } => RefreshExit::SignatureRefused,
        };
        (exit, error.to_string())
    })?;
    let sha: [u8; 32] = Sha256::digest(staged).into();
    let hex = hex(&sha);
    if record.admin_copy_sha256.as_deref() == Some(hex.as_str()) {
        return Ok(Replaced::Current);
    }
    let at_admin = fs
        .admin_copy_sha256()
        .map_err(|why| (RefreshExit::ReplaceFailed, why))?;
    if at_admin == Some(sha) {
        fs.write_sha256(&hex)
            .map_err(|why| (RefreshExit::RecordNotWritten, why))?;
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
        fs.log(
            true,
            &format!("the old copy stays in staging until the next run: {why}"),
        );
    }
    fs.write_sha256(&hex)
        .map_err(|why| (RefreshExit::RecordNotWritten, why))?;
    Ok(Replaced::Swapped)
}

/// The task's one argument: the action is the admin copy's path with this, nothing else.
pub const REFRESH_ARG: &str = "--refresh";
/// The task's execution limit: 5 minutes.
pub const EXECUTION_LIMIT: &str = "PT5M";

/// The descriptor of the task and its folder: protected; SYSTEM and Administrators full
/// control; the record owner `owner` read and run (`FRFX`), so the app can ask for a run and
/// can't change or delete the task.
pub fn task_sddl(owner: &Sid) -> String {
    format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{owner})")
}

/// The staging folder's owner and DACL: SYSTEM, alone, full control, inherited by what is made
/// inside; protected.
pub const STAGING_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)";
/// The admin copy's owner and DACL, given to the staged file before it moves into place:
/// Administrators own it; SYSTEM and Administrators have full control, Users read and run it;
/// protected. The installer's first copy (PR 1) is to carry the same, so a refresh changes no
/// access.
pub const ADMIN_COPY_SDDL: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;BU)";

/// Whether `found`, an owner and DACL read back as SDDL, is `expected`: the same text but for
/// the DACL's `AI` and `AR` flags, which Windows adds when it sets a descriptor through
/// `SetSecurityInfo` and which grant nothing.
pub fn sddl_matches(found: &str, expected: &str) -> bool {
    let plain = |sddl: &str| match sddl.split_once("D:") {
        Some((owner, dacl)) => {
            let flags = dacl.find('(').unwrap_or(dacl.len());
            let (flags, aces) = dacl.split_at(flags);
            let protected = if flags.contains('P') { "P" } else { "" };
            format!("{owner}D:{protected}{aces}")
        }
        None => sddl.to_owned(),
    };
    plain(found) == plain(expected)
}

/// Whether `a` and `b` name the same path, as Windows compares them: case folded, with any
/// `\\?\` prefix and either separator.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let fold = |path: &Path| {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.replace('/', "\\").to_lowercase()
    };
    fold(a) == fold(b)
}

/// `bytes` as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _infallible = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests;
