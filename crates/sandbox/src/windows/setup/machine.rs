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
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::sids::Sid;
use crate::windows::wfp::{NetworkReport, NetworkSetup};

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
    /// A folder in a pool user's profile (this part of its path) is a junction, a symbolic link
    /// or another reparse point, or isn't where `ProfileList` puts it: set-up changes nothing
    /// through it. The set-up can't repair that profile itself.
    #[error(
        "{0} in the sandbox user's profile isn't a plain folder, so set-up changed nothing \
         there: remove the sandbox users, then run set-up again"
    )]
    ProfileChanged(String),
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
    /// Grants the group `group` "Deny log on through Remote Desktop Services"
    /// (`SeDenyRemoteInteractiveLogonRight`, `LsaAddAccountRights`), so no pool user signs in
    /// over Remote Desktop; a right already held is `Ok`.
    fn deny_remote_logon(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// Takes that right off the group `group` (`LsaRemoveAccountRights`, this right only); a
    /// right not held is `Ok`.
    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError>;
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
    /// The folder Windows makes new profiles in (`ProfileList\ProfilesDirectory`, expanded):
    /// `C:\Users`.
    fn profiles_dir(&mut self) -> Result<PathBuf, MachineError>;
    /// The names of the entries directly in `dir`, a link listed as itself, never followed.
    fn folder_names(&mut self, dir: &Path) -> Result<Vec<String>, MachineError>;
    /// The owner of `path`, read without following a link (a pool user's `LocalLow`, `PR4h`).
    fn owner_of(&mut self, path: &Path) -> Result<Sid, MachineError>;
    /// Whether `path` is there, a junction or symbolic link counting as itself (never
    /// followed): `DeviceSync` is probed before its owner is read, and a reset's old profile
    /// folder after its delete (each `LocalLow` is read by [`Machine::owner_in_profile`]).
    fn exists(&mut self, path: &Path) -> Result<bool, MachineError>;
    /// Makes `BUILTIN\Administrators` the owner of `folder` alone (its owner only, nothing
    /// below it), so no pool user keeps an owner's implied `WRITE_DAC` on a folder whose denies
    /// protect what is below it. `folder` is reached from the pool user's profile `profile`
    /// (`ProfileList`'s path) by handle, one part at a time (`acl::profile::open_below`): a
    /// reparse point on the way is [`MachineError::ProfileChanged`], and nothing is changed.
    fn give_to_admins(&mut self, profile: &Path, folder: &Path) -> Result<(), MachineError>;
    /// The owner of `folder`, in the pool user's profile `profile`, read on the handle
    /// [`Machine::give_to_admins`] would reach it by; `None` when it is missing. A reparse
    /// point on the way is [`MachineError::ProfileChanged`].
    fn owner_in_profile(
        &mut self,
        profile: &Path,
        folder: &Path,
    ) -> Result<Option<Sid>, MachineError>;
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
    /// The pool users the owner's `matters.json` marks as owed a profile reset that no matter
    /// holds (`MatterSids::resets_due`). The engine calls it as the owner, once the lock is held,
    /// over a recorded pool. A missing file is none; one that can't be read is the error.
    fn resets_due(&mut self) -> Result<Vec<Sid>, MachineError>;
    /// Clears the reset mark of each of `users` in the owner's `matters.json`
    /// (`MatterSids::reset_done`), as the owner, once their profiles are made again and closed.
    fn reset_done(&mut self, users: &[Sid]) -> Result<(), MachineError>;
    /// The record's `ResetSids` (`record_values::RESET_SIDS`): the pool users the stop task
    /// found running, owed a profile reset even while a matter holds them. Read as this process,
    /// never as the owner: only SYSTEM and Administrators write it. A missing value is none.
    fn stop_resets(&mut self) -> Result<Vec<Sid>, MachineError>;
    /// Takes `users`, reset this run, off the record's `ResetSids`, as this process.
    fn stop_resets_done(&mut self, users: &[Sid]) -> Result<(), MachineError>;
    /// The record's `AdminCopyOwner`: who placed the admin copy, `None` when there is none.
    /// Read as this process.
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, MachineError>;
    /// Registers, or replaces, the SYSTEM tasks `\Clauseo\SandboxStop` and
    /// `\Clauseo\SandboxSweep` (`refresh::POOL_TASKS`) for the pool owner `owner`
    /// (`refresh::register_task`, as `--adopt` does per user), as this process.
    fn register_stop_task(&mut self, owner: &Sid) -> Result<(), MachineError>;
    /// Locks the admin copy's folder and everything in it again, as `--adopt` does
    /// (`admin_copy::os::lock_down_tree`), denying the pool's group `group`, as this process.
    fn lock_admin_copy(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// Registers, or replaces, the SYSTEM task `\Clauseo\SandboxWatch` for the pool owner
    /// `owner` (`refresh::register_task`), as this process. Only set-up registers it.
    fn register_watch_task(&mut self, owner: &Sid) -> Result<(), MachineError>;
    /// The users of `pool` the reuse rule would hold back once this run has reset `resets`, for
    /// the top-up's count (`top_up.rs`, `MatterSids::held_after_resets`), the owner's ledger
    /// opened trusting `trusted`; an input that can't be read holds every user. The engine calls
    /// it as the owner, once the lock is held, over a recorded pool. A missing `matters.json`
    /// holds none; one that can't be read is the error.
    fn held_users(
        &mut self,
        pool: &[Sid],
        trusted: &[Sid],
        resets: &[Sid],
    ) -> Result<Vec<Sid>, MachineError>;
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
    /// Set-up's network step (S15 PR7, `wfp::install`), elevated, not as the owner: the WFP
    /// provider, sublayer and block filters covering `setup`'s whole pool, the owner's grants,
    /// and the firewall rules. A WFP failure is the error; the firewall's is in the report.
    fn network(&mut self, setup: &NetworkSetup) -> Result<NetworkReport, MachineError>;
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
