//! The [`Machine`] seam: every call the set-up makes outside the permission writes, one method
//! per thing it asks Windows (or the owner's files) for. `WindowsMachine` (S15 `PR4e` to `PR4g`)
//! is the real one; the tests use a fake that records each call.

use std::io;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::windows::apply::Surroundings;
use crate::windows::fence::RegisteredMatter;
use crate::windows::ledger::Ledger;
use crate::windows::marker::{Marker, MarkerState};
use crate::windows::pool::MachineRecord;
use crate::windows::sids::Sid;

/// A pool user's password: wiped from memory when dropped.
pub type Password = Zeroizing<String>;

/// Why a [`Machine`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MachineError {
    /// A Windows call failed with this error code.
    #[error("{call} failed (error {code})")]
    Os {
        /// The call, as Windows names it (`NetUserAdd`).
        call: &'static str,
        /// Its error code (`GetLastError`, or the `NET_API_STATUS`).
        code: u32,
    },
    /// Another set-up holds the machine-wide set-up lock.
    #[error("another set-up is running on this computer")]
    LockHeld,
    /// The call isn't built yet: `WindowsMachine` gained its methods over S15 `PR4e` to `PR4h`.
    #[error("{0} isn't built yet")]
    Unsupported(&'static str),
    /// Anything else; what failed.
    #[error("{0}")]
    Other(String),
}

/// How the Secondary Logon service (`seclogon`) starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStart {
    /// It starts with Windows.
    Automatic,
    /// It starts when asked, as it does on a default install.
    Manual,
    /// It never starts: no pool user can be logged on through it.
    Disabled,
}

/// Whether Windows' local policy lets the `Users` group log on interactively, as far as a check
/// short of a logon can tell (the logon in step 6 is the real test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogonRight {
    /// Nothing refuses it.
    Allowed,
    /// "Deny log on locally" (`SeDenyInteractiveLogonRight`) names `Users`.
    Refused,
}

/// The profile a logon loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDir {
    /// The profile's folder: `C:\Users\ClauseoSbNN`.
    pub path: PathBuf,
    /// Windows gave a temporary profile (its `ProfileList` state says so, or the folder is
    /// under `C:\Users\TEMP`): the `LocalLow` the deny would close is not the user's own.
    pub temporary: bool,
}

/// Everything the set-up asks of the machine besides the permission writes
/// ([`AclWriter`](crate::windows::AclWriter)), one method per step of
/// [`set_up`](super::set_up). Each is one Windows call or one file the owner keeps, so the
/// engine's order and rollback are tested on every OS against a fake.
///
/// Account methods take the account's name (`ClauseoSbNN`), as the `Net*` calls do; the profile
/// methods take its SID, as `DeleteProfileW` does.
pub trait Machine {
    /// Takes the machine-wide set-up lock (a named mutex in a private namespace bounded by
    /// Administrators). Another set-up holding it is [`MachineError::LockHeld`]. For `PR4e`:
    /// `WAIT_ABANDONED` (a set-up that died
    /// holding it) counts as acquired: the set-up that died stopped between two of its steps,
    /// and the next run starts from what the record says.
    fn lock(&mut self) -> Result<(), MachineError>;
    /// Gives the lock back. Called once for every `lock` that succeeded.
    fn unlock(&mut self);
    /// `HKLM\SOFTWARE\Clauseo\Sandbox`, whole; `None` when the key is missing.
    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError>;
    /// Writes `record` as the whole key, replacing every value.
    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError>;
    /// Deletes the key: only when a fresh set-up rolls back to nothing.
    fn delete_record(&mut self) -> Result<(), MachineError>;
    /// How the Secondary Logon service starts.
    fn seclogon(&mut self) -> Result<ServiceStart, MachineError>;
    /// Whether local policy refuses the interactive logon right to `Users`.
    fn logon_right(&mut self) -> Result<LogonRight, MachineError>;
    /// The SID of the account named `name`, if one exists.
    fn account_sid(&mut self, name: &str) -> Result<Option<Sid>, MachineError>;
    /// The name of the account whose SID is `sid`, if one exists.
    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError>;
    /// The SID of the local group `ClauseoSandboxUsers`, if it exists.
    fn group(&mut self) -> Result<Option<Sid>, MachineError>;
    /// Makes the local group `ClauseoSandboxUsers`; its SID.
    fn create_group(&mut self) -> Result<Sid, MachineError>;
    /// How many members the group `group` has.
    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError>;
    /// Deletes the group `group`.
    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// A new random password for a pool user (`PR4e`'s generator).
    fn new_password(&mut self) -> Password;
    /// Makes the local account `name` with `password`, `UF_DONT_EXPIRE_PASSWD |
    /// UF_PASSWD_CANT_CHANGE`; its SID. An account by that name already there is an error,
    /// never adopted (D-B5).
    fn create_user(&mut self, name: &str, password: &Password) -> Result<Sid, MachineError>;
    /// Sets `name`'s password to `password` (a re-run, D-B6).
    fn set_password(&mut self, name: &str, password: &Password) -> Result<(), MachineError>;
    /// Puts `name` in the group `group`, and in `Users` where the account isn't already; a
    /// membership already there is `Ok`.
    fn add_to_group(&mut self, name: &str, group: &Sid) -> Result<(), MachineError>;
    /// Hides `name` from the sign-in screen (its `SpecialAccounts\UserList` value).
    fn hide(&mut self, name: &str) -> Result<(), MachineError>;
    /// Deletes the account `name` and its sign-in screen value.
    fn delete_user(&mut self, name: &str) -> Result<(), MachineError>;
    /// Logs `name` on once with `password`, its profile loaded (made from Default the first
    /// time, `AppData\LocalLow` included), then unloads it. `ERROR_LOGON_TYPE_NOT_GRANTED`
    /// (1385) is [`MachineError::Os`] with that code.
    fn log_on_once(&mut self, name: &str, password: &Password) -> Result<ProfileDir, MachineError>;
    /// The profile Windows lists for `sid` (`ProfileList`), if any: for a pool user an extend
    /// doesn't log on.
    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError>;
    /// Deletes `sid`'s profile (`DeleteProfileW`).
    fn delete_profile(&mut self, sid: &Sid) -> Result<(), MachineError>;
    /// The owner of `path`, read without following a link (a pool user's `LocalLow`, `PR4h`).
    fn owner_of(&mut self, path: &Path) -> Result<Sid, MachineError>;
    /// Whether `path` is there, a junction or symbolic link counting as itself (never
    /// followed): `DeviceSync` and each `LocalLow` are probed before their owners are read.
    fn exists(&mut self, path: &Path) -> Result<bool, MachineError>;
    /// Makes `BUILTIN\Administrators` the owner of `path` alone (its owner only, nothing below
    /// it, never following a link), so no pool user keeps an owner's implied `WRITE_DAC` on a
    /// folder whose denies protect what is below it.
    fn give_to_admins(&mut self, path: &Path) -> Result<(), MachineError>;
    /// The account the set-up process itself runs as (the elevated token's user), which may own
    /// a profile folder it loaded; `None` when it couldn't be read.
    fn own_account(&self) -> Option<&Sid>;
    /// `%ProgramData%`, which holds `Microsoft\DeviceSync`.
    fn program_data(&mut self) -> Result<PathBuf, MachineError>;
    /// The owner's `setup.json`, read as `marker::read` reads it for the owner.
    fn read_marker(&mut self) -> MarkerState;
    /// Writes the owner's `setup.json`, as the owner.
    fn write_marker(&mut self, marker: &Marker) -> Result<(), MachineError>;
    /// Whether any matter of the owner's has an open session (the set-aside record's
    /// open-sessions sets).
    fn sessions_open(&mut self) -> Result<bool, MachineError>;
    /// The owner's registered matters for step 8: each matter `matters.json` lists as holding
    /// a pool user, with the folders its ledger entries name (`registered.rs`), the ledger
    /// trusting `trusted`. The engine calls it as the owner, once the lock is held. A missing
    /// `matters.json` is no matters; one that can't be read is the error.
    fn registered_matters(
        &mut self,
        trusted: &[Sid],
    ) -> Result<Vec<RegisteredMatter>, MachineError>;
    /// The owner's ledger, `<data_dir>\sandbox\acl-ledger.json`, opened trusting `trusted` (the
    /// pool users and the group) against this machine's reserved accounts.
    fn ledger(&mut self, trusted: &[Sid]) -> io::Result<Ledger>;
    /// What the fence's walks need from around the matters (the folder check, the set-aside).
    fn surroundings(&mut self) -> &mut dyn Surroundings;
    /// The volumes whose change journal set-up enlarges (D-B12): the fixed NTFS volumes holding
    /// any of `roots`, `%SystemDrive%` and the profiles' volume; never a removable one.
    fn journal_volumes(&mut self, roots: &[&Path]) -> Result<Vec<PathBuf>, MachineError>;
    /// The change journal's `MaximumSize` on `volume`; `None` when it has no journal.
    fn journal_size(&mut self, volume: &Path) -> Result<Option<u64>, MachineError>;
    /// Creates or enlarges `volume`'s change journal to `max` bytes, growing by `delta`.
    fn grow_journal(&mut self, volume: &Path, max: u64, delta: u64) -> Result<(), MachineError>;
    /// Writes `<data_dir>\sandbox\secrets.bin` as the owner, protected for the owner: each of
    /// `secrets`, and for every other SID of `pool` the blob the file holds now (an extend
    /// leaves the existing users' passwords as they are).
    fn protect_for_owner(
        &mut self,
        secrets: &[(Sid, Password)],
        pool: &[Sid],
    ) -> Result<(), MachineError>;
    /// The time now, in seconds since the Unix epoch, for the marker.
    fn now(&mut self) -> u64;
    /// The owner the machine acts as for the owner's files: the verified parent's token user
    /// (`Owner::sid`, D-B2). [`set_up`](super::set_up) refuses a request for anyone else.
    fn owner(&self) -> &Sid;
    /// Runs `f` with the calling thread acting as [`Machine::owner`] (`Owner::run_as`), then as
    /// itself again. Every call that reads or writes under `<data_dir>\sandbox\` runs inside
    /// it: `current_user_sid` follows the thread's token, so outside it an over-the-shoulder
    /// run would put the administrator's SID in each protected DACL and shut the owner out.
    /// Called again inside `f`, it runs `f` as it is. A failed impersonation is the `Err`, and
    /// `f` doesn't run.
    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError>
    where
        Self: Sized;
}

/// `f` run through [`Machine::as_owner`], a failed impersonation folded into `f`'s own error.
pub(super) fn owned<M: Machine, T>(
    machine: &mut M,
    f: impl FnOnce(&mut M) -> Result<T, MachineError>,
) -> Result<T, MachineError> {
    machine.as_owner(f).and_then(|inner| inner)
}
