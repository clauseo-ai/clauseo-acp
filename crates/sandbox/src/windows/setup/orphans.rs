//! A killed set-up's orphans (review L3). Set-up makes a pool user's account, puts it in the
//! pool's group and hides it, and only then records its SID in `PoolSids`; a run ended in
//! between (`schtasks /end`, a crash) leaves an account no record names. Such an account holds
//! its pool name, and set-up skips a name an account outside the record holds (D-B5), so runs
//! killed again and again would use up the 32 names. When set-up reaches such a name it takes
//! the account back: deleted, and the name made again.
//!
//! The rule ([`is_orphan`]) never deletes an account that isn't plainly one set-up made: its
//! name is a pool user's (`ClauseoSbNN`), it is in the pool's group, no SID of the record is
//! it, and it passes the stop's and removal's check of a pool SID (`strays::check_pool_sid`: a
//! local account of this computer, as the group is, not built in, not this process's user nor
//! the owner). An account that fails any part (a stranger's, or one killed before its group
//! was set) stays skipped. It was never logged on (the logon comes after the record), so it
//! has no profile to delete.
//!
//! A first set-up ended before its record leaves its group with no record, which set-up would
//! refuse as someone else's for good. [`take_back_group`] reuses it when it is plainly set-up's:
//! its comment is the one set-up writes and every member is an orphan by the same rule.

use super::machine::{GROUP_COMMENT, Machine, MachineError};
use super::strays::check_pool_sid;
use crate::windows::pool::pool_number;
use crate::windows::sids::Sid;

/// What set-up knows of an account found under a pool name it is about to use.
#[derive(Debug, Clone, Copy)]
pub(super) struct Found<'a> {
    /// The name it was found under.
    pub(super) name: &'a str,
    /// Its SID.
    pub(super) sid: &'a Sid,
    /// Whether it is in `group`.
    pub(super) in_group: bool,
}

/// Whether `found` is a killed run's orphan, as the module says: `group` the pool's group,
/// `recorded` every SID of the record, `own` this process's user and `owner` the owner.
pub(super) fn is_orphan(
    found: Found<'_>,
    group: &Sid,
    recorded: &[Sid],
    own: &Sid,
    owner: &Sid,
) -> bool {
    let Some(machine) = group.domain() else {
        return false;
    };
    pool_number(found.name).is_some()
        && found.in_group
        && !recorded.contains(found.sid)
        && check_pool_sid(found.sid, machine, own, owner, Some(found.name)).is_ok()
}

/// Takes back the account `sid` found under the pool name `name` when it is an orphan
/// ([`is_orphan`]): whether it was deleted, so the name is free. A failed read or delete is
/// logged and leaves the name skipped, as before.
pub(super) fn reclaim(
    machine: &mut impl Machine,
    name: &str,
    sid: &Sid,
    group: &Sid,
    recorded: &[Sid],
) -> bool {
    // The cheap rules first: a stranger's account is never asked about.
    let owner = machine.owner().clone();
    let own = machine
        .own_account()
        .cloned()
        .unwrap_or_else(|| owner.clone());
    let shaped = Found {
        name,
        sid,
        in_group: true,
    };
    if !is_orphan(shaped, group, recorded, &own, &owner) {
        return false;
    }
    let in_group = match machine.in_group(sid, group) {
        Ok(in_group) => in_group,
        Err(error) => {
            tracing::warn!(%name, %error, "a pool name's account: its groups can't be read");
            return false;
        }
    };
    if !is_orphan(Found { in_group, ..shaped }, group, recorded, &own, &owner) {
        return false;
    }
    match machine.delete_user(name) {
        Ok(()) => {
            tracing::info!(%name, "deleted a sandbox user a set-up made but never recorded");
            true
        }
        Err(error) => {
            tracing::warn!(%name, %error, "a sandbox user no record names couldn't be deleted");
            false
        }
    }
}

/// Whether `group`, found under the pool group's name while no record exists (a first set-up
/// ended before its record was written), is one set-up made and may be reused: its comment is
/// ours ([`GROUP_COMMENT`]) and every member is a killed run's orphan ([`is_orphan`], with no
/// SID recorded), or it has none. Then each member is deleted and it is `true`; any other
/// comment or member (an administrator's group) is `false`, refused as before, with nothing
/// deleted.
pub(super) fn take_back_group(
    machine: &mut impl Machine,
    group: &Sid,
) -> Result<bool, MachineError> {
    let facts = machine.group_facts(group)?;
    if facts.comment != GROUP_COMMENT {
        tracing::warn!("a group with the pool group's name has another comment; not ours");
        return Ok(false);
    }
    let owner = machine.owner().clone();
    let own = machine
        .own_account()
        .cloned()
        .unwrap_or_else(|| owner.clone());
    let mut orphans = Vec::new();
    for sid in &facts.members {
        let name = machine.account_name(sid)?;
        let found = name.as_deref().map(|name| Found {
            name,
            sid,
            in_group: true,
        });
        if !found.is_some_and(|found| is_orphan(found, group, &[], &own, &owner)) {
            tracing::warn!("the pool group's name holds an account set-up didn't make; not ours");
            return Ok(false);
        }
        orphans.extend(name);
    }
    for name in &orphans {
        machine.delete_user(name)?;
        tracing::info!(%name, "deleted a sandbox user a first set-up made but never recorded");
    }
    Ok(true)
}
