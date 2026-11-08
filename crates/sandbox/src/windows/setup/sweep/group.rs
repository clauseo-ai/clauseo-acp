//! Group-principal tasks (step 4, `sweep.rs`). A task whose principal is a group (its
//! `GroupId`, or a `UserId` that resolves to a group's SID) runs for the group's members. The
//! sweep doesn't rely on Windows refusing a standard user's registration of one: such a task is
//! judged by the owner its security descriptor names, read as every task's owner is
//! (`GetSecurityDescriptor`). A pool owner is the pool's task, deleted, and 56 when it can't
//! be; an owner that can't be read (unreadable, ownerless, over its cap) is 56; any other owner
//! (an administrator, SYSTEM, another user) is ignored. A task SYSTEM's listing hides
//! (`unlisted.rs`) is 56 whatever its group, its cache key's `SD` or its file says. Whether
//! the group may hold a pool user ([`Groups::can_contain`]) changes none of this: with a pool
//! owner or an unreadable one never accepted, the answer doesn't matter. It is counted in the
//! log line, so an administrator sees how many tasks a pool user could run were their owner
//! the pool's.
//!
//! A task's `Author` (`RegistrationInfo`) is never trusted: whoever registers the task writes
//! it, so it proves nothing. It is logged for a group task left at 56, and nothing more.

use std::collections::BTreeMap;

use super::{Failed, Principal, Resolved, Sweeper, may_be_pool};
use crate::windows::sids::Sid;

/// The well-known groups a pool user's token holds, or may: Everyone, Authenticated Users,
/// INTERACTIVE, Users, LOCAL and CONSOLE LOGON.
pub const POOL_GROUPS: [&str; 6] = [
    "S-1-1-0",
    "S-1-5-11",
    "S-1-5-4",
    "S-1-5-32-545",
    "S-1-2-0",
    "S-1-2-1",
];

/// SYSTEM, LOCAL SERVICE and NETWORK SERVICE: one identity each, never a group's members,
/// though `LookupAccountSidW` calls them well-known groups.
const SERVICES: [&str; 3] = ["S-1-5-18", "S-1-5-19", "S-1-5-20"];

/// What the run knows of groups, each read at most once a run.
pub(super) struct Groups {
    /// The record's `GroupSid` (`ClauseoSandboxUsers`); `Err` when it can't be read, and then
    /// every group may hold a pool user.
    pool_group: Result<Option<Sid>, Failed>,
    /// Whether each SID asked about is a group's ([`Sweeper::is_group`]).
    kinds: BTreeMap<Sid, bool>,
    /// Whether each group asked about may hold a pool user ([`Groups::can_contain`]).
    contain: BTreeMap<Sid, bool>,
}

impl Groups {
    /// The run's groups, the record's `GroupSid` read.
    pub(super) fn of(fs: &mut impl Sweeper) -> Groups {
        Groups {
            pool_group: fs.pool_group(),
            kinds: BTreeMap::new(),
            contain: BTreeMap::new(),
        }
    }

    /// `None` when the task whose principal is `principal` isn't a group's
    /// ([`Groups::is_group_task`]); otherwise whether its group may hold a pool user
    /// ([`Groups::can_contain`]), which a name that resolves to no SID may. For the log alone.
    pub(super) fn group_task(
        &mut self,
        fs: &mut impl Sweeper,
        principal: &Principal,
        resolved: &BTreeMap<String, Resolved>,
        pool: &[Sid],
    ) -> Option<bool> {
        if !self.is_group_task(fs, principal, resolved, pool) {
            return None;
        }
        Some(sid_of(principal, resolved).is_none_or(|sid| self.can_contain(fs, sid, pool)))
    }

    /// Whether the task whose principal is `principal` (its name already resolved, in
    /// `resolved`) is a group's: its `GroupId` alone, or a name that resolves to a group's SID.
    /// The service accounts and the pool's SIDs are users; a SID whose kind can't be read
    /// counts as a group's.
    fn is_group_task(
        &mut self,
        fs: &mut impl Sweeper,
        principal: &Principal,
        resolved: &BTreeMap<String, Resolved>,
        pool: &[Sid],
    ) -> bool {
        if principal.user_id.is_empty() && !principal.group_id.is_empty() {
            return true;
        }
        let Some(sid) = sid_of(principal, resolved) else {
            return false;
        };
        if pool.contains(sid) || SERVICES.contains(&sid.as_str()) {
            return false;
        }
        if self.named(sid) {
            return true;
        }
        if let Some(known) = self.kinds.get(sid) {
            return *known;
        }
        let group = fs.is_group(sid).unwrap_or(true);
        self.kinds.insert(sid.clone(), group);
        group
    }

    /// Whether a pool user may be one of the members of the group `sid`: a pool SID, one of
    /// [`POOL_GROUPS`], the record's `GroupSid` (or any group, when that can't be read), or a
    /// local group whose members ([`Sweeper::group_members`]) hold one of those; a lookup that
    /// fails counts as yes. A group no pool user's token may hold ([`may_be_pool`]:
    /// Administrators, SYSTEM, a service's, another domain's) can't.
    pub(super) fn can_contain(&mut self, fs: &mut impl Sweeper, sid: &Sid, pool: &[Sid]) -> bool {
        if pool.contains(sid) || self.named(sid) || self.pool_group.is_err() {
            return true;
        }
        if !may_be_pool(Some(sid), pool) {
            return false;
        }
        if let Some(known) = self.contain.get(sid) {
            return *known;
        }
        let can = match fs.group_members(sid) {
            Ok(members) => members
                .iter()
                .any(|member| pool.contains(member) || self.named(member)),
            Err(_) => true,
        };
        self.contain.insert(sid.clone(), can);
        can
    }

    /// Whether `sid` is one of [`POOL_GROUPS`] or the record's `GroupSid`.
    fn named(&self, sid: &Sid) -> bool {
        POOL_GROUPS.contains(&sid.as_str())
            || matches!(&self.pool_group, Ok(Some(group)) if group == sid)
    }
}

/// The SID `principal`'s name resolved to, in `resolved`; `None` when it resolved to none.
pub(super) fn sid_of<'r>(
    principal: &Principal,
    resolved: &'r BTreeMap<String, Resolved>,
) -> Option<&'r Sid> {
    match resolved.get(principal.name()?) {
        Some(Resolved::Sid(sid)) => Some(sid),
        _ => None,
    }
}
