//! The [`Removal`] seam: every call `--remove` makes outside the permission writes. Account
//! methods take the account's name (`ClauseoSbNN`), profile methods its SID, as set-up's
//! `Machine` does. `WindowsRemoval` (`windows_removal.rs`) is the real one; the tests wrap
//! the set-up's fake machine.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::FolderOwner;
use crate::windows::apply::Surroundings;
use crate::windows::ledger::Ledger;
use crate::windows::pool::MachineRecord;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::setup::{MachineError, ProfileDir};
use crate::windows::sids::Sid;

/// Everything a removal asks of the machine besides the permission writes, one method per
/// thing it asks Windows (or the owner's files) for.
pub trait Removal {
    /// Takes the machine-wide set-up lock, waiting up to `wait_ms` milliseconds; still held by
    /// another set-up then is [`MachineError::LockHeld`]. Abandoned counts as acquired.
    fn lock_within(&mut self, wait_ms: u32) -> Result<(), MachineError>;
    /// Gives the lock back. Called once for every `lock_within` that succeeded.
    fn unlock(&mut self);
    /// `HKLM\SOFTWARE\Clauseo\Sandbox`, whole; `None` when the key is missing.
    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError>;
    /// Writes `record` as the whole key.
    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError>;
    /// Deletes the key; one already gone is `Ok`.
    fn delete_record(&mut self) -> Result<(), MachineError>;
    /// The record's `AdminCopyOwner`: who placed the admin copy, `None` when there is none.
    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, MachineError>;
    /// Deletes the SYSTEM tasks `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep`
    /// (`refresh::POOL_TASKS`, `refresh::unregister_task`); one already gone is `Ok`.
    fn unregister_stop_task(&mut self) -> Result<(), MachineError>;
    /// The user who asked: the token user of the parent Windows recorded for this process,
    /// checked against ID reuse (`Owner::of_parent`, D-B2). `None` when it can't be found.
    fn invoker(&self) -> Option<&Sid>;
    /// Whether [`Removal::as_owner`] can act as the invoker with a token no stronger than a
    /// standard user's (`remove::acting_token`): the parent's own when it isn't elevated, else
    /// its lowered copy. `false` when there is no invoker or lowering failed.
    fn can_act_as_owner(&self) -> bool;
    /// Stops every process whose token user is one of `pool` (D-B13), refusing the whole call
    /// when a SID can't be a pool user's (`processes::check_pool`; `owner` is the record's).
    /// A process that won't stop is logged, not an error: the profile delete then fails, named.
    /// How many it couldn't stop (failed, or not exited when the wait ran out).
    fn stop_strays(&mut self, pool: &[Sid], owner: &Sid) -> Result<usize, MachineError>;
    /// Removes the network filters, the owner's read grant on them and the firewall rules
    /// (S15 PR7, `wfp::remove`); what is already gone counts as removed. A WFP failure is the
    /// `Err`; `Some` names a firewall failure, which isn't fatal.
    fn remove_network(&mut self, owner: &Sid) -> Result<Option<String>, MachineError>;
    /// The name of the account whose SID is `sid`, if one exists.
    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError>;
    /// The profile Windows lists for `sid` (`ProfileList`), if any.
    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError>;
    /// Whether `folder` in the profile `profile` is there, reached by handle from the profile
    /// as `Machine::in_profile` reaches it: a reparse point on the way is
    /// `MachineError::ProfileChanged`.
    fn in_profile(&mut self, profile: &Path, folder: &Path) -> Result<bool, MachineError>;
    /// Deletes `sid`'s profile (`DeleteProfileW`), refusing a SID that can't be a pool user's
    /// (`owner` is the record's); none is `Ok`. Its links go first, each deleted itself
    /// (`profile::delete_profile`).
    fn delete_profile(&mut self, sid: &Sid, owner: &Sid) -> Result<(), MachineError>;
    /// Deletes the account `name` and its sign-in screen value; either gone already is `Ok`.
    fn delete_user(&mut self, name: &str) -> Result<(), MachineError>;
    /// `%ProgramData%`, which holds `Microsoft\DeviceSync`.
    fn program_data(&mut self) -> Result<PathBuf, MachineError>;
    /// The SID of the local group `ClauseoSandboxUsers`, if it exists.
    fn group(&mut self) -> Result<Option<Sid>, MachineError>;
    /// How many members the group `group` has.
    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError>;
    /// Deletes the group `group`.
    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// Takes "Deny log on through Remote Desktop Services" (`SeDenyRemoteInteractiveLogonRight`)
    /// off the group `group` (`LsaRemoveAccountRights`, this right only); not held is `Ok`.
    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// Runs `f` with the calling thread acting as [`Removal::invoker`] (`Owner::run_as`), with
    /// the token [`Removal::can_act_as_owner`] names, then as itself again; called again inside
    /// `f`, runs `f` as it is. No invoker, or a failed impersonation, is the `Err`, and `f`
    /// doesn't run; so is a parent whose token couldn't be lowered. The removal calls it only
    /// when the invoker is the record's owner.
    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError>
    where
        Self: Sized;
    /// As the owner (it takes [`Removal::as_owner`] itself when called outside it): the
    /// record's `data_dir` checked as `--data-dir` is (a plain drive path; resolved with no
    /// junction or link on the way; owned as `remove::owners` allows), then the ledger in its final
    /// path, trusting `trusted`. `None` when the folder or the ledger is missing: nothing to
    /// take back. Sets [`Removal::surroundings`] to that folder.
    fn owners_ledger(
        &mut self,
        data_dir: &Path,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError>;
    /// What `forget_matter`'s walks need, for the folder [`Removal::owners_ledger`] checked.
    fn surroundings(&mut self) -> &mut dyn Surroundings;
    /// As the owner, inside [`Removal::as_owner`] and only there, once `matter` is forgotten:
    /// its set-aside folder in the folder [`Removal::owners_ledger`] checked deleted with its
    /// bookkeeping when it holds nothing else and each file is fit to go, else left as it is
    /// (`set_aside::forget_bookkeeping`, by handle, never through a link). Called outside it,
    /// refused with nothing touched. The administrator's path never calls it: an administrator
    /// deletes nothing in the owner's folders (K1). Nothing checked yet is `Ok`, nothing touched.
    fn forget_set_aside(&mut self, matter: &MatterId) -> Result<(), MachineError>;
    /// As this process, for a per-machine removal the owner didn't ask for: the record's
    /// `data_dir` checked as [`Removal::owners_ledger`] checks it, with the folder and its
    /// `sandbox` folder owned as `remove::owners` allows for `owner` (the record's), then the
    /// ledger read, and never written. `None` when the folder or the ledger is missing.
    fn admins_ledger(
        &mut self,
        data_dir: &Path,
        owner: &Sid,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError>;
    /// As this process: whether the ledger's `path` may have an entry removed by a removal the
    /// owner didn't ask for, and who owns it. `Ok` only for a folder reached with no junction
    /// or link on the way whose final path is `path`, owned as `owners::matter_folder` allows
    /// (`owner`, the record's, or Administrators or SYSTEM), and passing the gate's folder check
    /// (a local volume that keeps permissions).
    fn admin_target(&mut self, path: &Path, owner: &Sid) -> Result<FolderOwner, MachineError>;
    /// Whether the record's `owner` is in Administrators, directly or through a group; `false`
    /// when that can't be read.
    fn owner_in_administrators(&mut self, owner: &Sid) -> bool;
}
