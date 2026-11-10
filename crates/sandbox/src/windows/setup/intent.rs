//! The intent record (gap a of the kill-safety review) and a partial record (gap b): what an
//! interrupted run leaves in `HKLM\SOFTWARE\Clauseo\Sandbox`, and how the next set-up and
//! `--remove` finish it.
//!
//! Before set-up makes the group or any account, it writes its intent into the record key:
//! `OwnerSid` (when the key has none), [`PENDING_GROUP`] before the group, [`PENDING_USERS`]
//! (the one pool name it is about to make) before each account. Once the record names what was
//! made, the pending values are deleted. Each account is made with Clauseo's comment
//! (`accounts::USER_COMMENT`), and the group has carried its own since it was first made, so
//! either can be told apart from an account that only shares a name.
//!
//! A run ended in between (a kill, a crash, the deadline, power loss) leaves the pending values.
//! The next set-up (`early_steps::recover`) and `--remove` undo them with [`undo`]: an account
//! the intent names is deleted only when it is there, is ours (our comment, or a member of our
//! group) and the record doesn't name it; the group only when it is there, carries our comment
//! and isn't the record's. No account or group the intent doesn't name is ever touched.
//!
//! A record missing `OwnerSid`, `GroupSid` or `DataDir` (an intent alone, or a first record
//! write that stopped part way) reads as no record (`record_values::decode`) and as
//! [`Intent::partial`]: set-up finishes what it names and starts again, and `--remove`
//! (`--per-machine`, or the owner it names) cleans it; neither is blocked by it.
//!
//! Pure: built on every OS so its tests run everywhere.

use super::machine::{Machine, MachineError};
use super::outcome::cause_of;
use super::record_values::RegValue;
use super::remove::Removal;
use crate::windows::pool::{MachineRecord, pool_number};
use crate::windows::sids::Sid;

/// `PendingGroup` (`REG_DWORD` 1): set-up is about to make the group `ClauseoSandboxUsers`,
/// which no record names yet.
pub const PENDING_GROUP: &str = "PendingGroup";
/// `PendingUsers` (`REG_MULTI_SZ`): the pool names set-up is about to make, which no record
/// names yet.
pub const PENDING_USERS: &str = "PendingUsers";

/// What the record key says a run was about to make, and whether its record is partial.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Intent {
    /// `OwnerSid` as read, from a whole record or not.
    pub owner: Option<Sid>,
    /// `PendingGroup`.
    pub group: bool,
    /// `PendingUsers`, each a pool name, once.
    pub users: Vec<String>,
    /// The key holds some of the set-up's values but not a whole record.
    pub partial: bool,
}

impl Intent {
    /// Whether there is nothing to finish: nothing pending and no partial record.
    pub fn is_empty(&self) -> bool {
        !self.group && self.users.is_empty() && !self.partial
    }
}

/// The pool names in `listed` (`PendingUsers` as read), each once, in order; anything else is
/// dropped, so no name outside the pool is ever acted on.
pub fn pool_names(listed: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in listed {
        if pool_number(&name).is_some() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// `names` as `PendingUsers`' `REG_MULTI_SZ`: each name's units and a NUL, then one more NUL.
pub fn names_value(names: &[String]) -> RegValue {
    let mut units: Vec<u16> = names
        .iter()
        .flat_map(|name| name.encode_utf16().chain([0]))
        .collect();
    units.push(0);
    RegValue::MultiSz(units)
}

/// The pool names in `PendingUsers`' units, as [`pool_names`] keeps them; a string that isn't
/// UTF-16 is dropped with the rest that aren't pool names.
pub fn names_from_units(units: &[u16]) -> Vec<String> {
    pool_names(
        units
            .split(|unit| *unit == 0)
            .filter_map(|name| String::from_utf16(name).ok()),
    )
}

/// What [`undo`] took away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Undone {
    /// Accounts deleted.
    pub users: usize,
    /// Whether the group was deleted.
    pub group: bool,
}

/// The calls [`undo`] makes, which set-up's [`Machine`] and the removal's [`Removal`] both have.
pub trait Leftovers {
    /// The SID of the account `name` when it is there and ours (Clauseo's comment, or a member
    /// of the group `ClauseoSandboxUsers`); `None` otherwise.
    fn marked_user(&mut self, name: &str) -> Result<Option<Sid>, MachineError>;
    /// The SID of the group `ClauseoSandboxUsers` when it is there and carries its comment.
    fn marked_group(&mut self) -> Result<Option<Sid>, MachineError>;
    /// Deletes the account `name` and its sign-in screen value.
    fn delete_user(&mut self, name: &str) -> Result<(), MachineError>;
    /// Takes "Deny log on through Remote Desktop Services" off the group `group`.
    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError>;
    /// Deletes the group `group`.
    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError>;
}

/// Undoes what `intent` names and `record` (the whole record, when there is one) doesn't, as
/// the module says: the accounts first, then the group, its Remote Desktop deny taken off
/// before it goes. The first failure stops it, naming what failed; the intent stays then, for
/// the next run. Never an account named by the record, or named `owner`'s.
pub fn undo(
    seam: &mut impl Leftovers,
    intent: &Intent,
    record: Option<&MachineRecord>,
) -> Result<Undone, String> {
    let mut undone = Undone::default();
    let owner = record.map(|r| &r.owner).or(intent.owner.as_ref());
    for name in pool_names(intent.users.iter().cloned()) {
        let failed = |error: MachineError| format!("{name}: {}", cause_of(&error));
        let Some(sid) = seam.marked_user(&name).map_err(failed)? else {
            // Gone, or not ours: never touched.
            continue;
        };
        if record.is_some_and(|r| r.pool.contains(&sid)) || owner == Some(&sid) {
            continue;
        }
        seam.delete_user(&name).map_err(failed)?;
        undone.users += 1;
    }
    if intent.group {
        let failed = |error: MachineError| format!("the group: {}", cause_of(&error));
        if let Some(group) = seam.marked_group().map_err(failed)?
            && record.is_none_or(|r| r.group != group)
        {
            seam.remove_remote_logon_deny(&group).map_err(failed)?;
            seam.delete_group(&group).map_err(failed)?;
            undone.group = true;
        }
    }
    Ok(undone)
}

/// Set-up's [`Machine`] as [`Leftovers`].
pub(super) struct SetUpSide<'m, M: Machine>(pub(super) &'m mut M);

impl<M: Machine> Leftovers for SetUpSide<'_, M> {
    fn marked_user(&mut self, name: &str) -> Result<Option<Sid>, MachineError> {
        self.0.marked_user(name)
    }
    fn marked_group(&mut self) -> Result<Option<Sid>, MachineError> {
        self.0.marked_group()
    }
    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        self.0.delete_user(name)
    }
    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.0.remove_remote_logon_deny(group)
    }
    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.0.delete_group(group)
    }
}

/// The removal's [`Removal`] as [`Leftovers`].
pub(super) struct RemovalSide<'m, R: Removal>(pub(super) &'m mut R);

impl<R: Removal> Leftovers for RemovalSide<'_, R> {
    fn marked_user(&mut self, name: &str) -> Result<Option<Sid>, MachineError> {
        self.0.marked_user(name)
    }
    fn marked_group(&mut self) -> Result<Option<Sid>, MachineError> {
        self.0.marked_group()
    }
    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        self.0.delete_user(name)
    }
    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.0.remove_remote_logon_deny(group)
    }
    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.0.delete_group(group)
    }
}
