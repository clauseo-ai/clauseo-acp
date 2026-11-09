//! Set-up's local grant of "Allow log on locally" (`SeInteractiveLogonRight`) to the group
//! `ClauseoSandboxUsers`, and `--remove`'s undo of it (0049, "Logon rights").
//!
//! Set-up runs elevated, once the group exists (step 3), before any pool user logs on:
//!
//! - policy ([`rights_policy`](super::rights_policy)) defines the right and includes the pool:
//!   nothing to do;
//! - policy defines it without the pool: no grant (a policy refresh would undo it), and the
//!   set-up fails with [`LOGON_POLICY_EXCLUDES`](super::LOGON_POLICY_EXCLUDES), which is
//!   `Blocked::LogonPolicy`;
//! - policy can't be read ([`RightPolicy::Unknown`]): no grant; the logon is the test, and the
//!   next set-up checks again;
//! - no policy defines it: when the local holders ([`LogonRights::holders`]) already name the
//!   group, `Users`, `Everyone` or `Authenticated Users`, nothing; else the record's
//!   `LogonRightAdded` is written with the group's SID, then the right is added to the group.
//!
//! `--remove` takes the right off only the SID `LogonRightAdded` names, so a right IT granted
//! stays. Nothing here grants any right but [`GRANTED_RIGHTS`]: never
//! "Allow log on through Remote Desktop Services".

use super::machine::MachineError;
use super::rights_policy::{INTERACTIVE_RIGHT, PolicyState, Pool, RightPolicy};
use crate::windows::sids::Sid;

/// Every right set-up may grant: "Allow log on locally" alone.
pub const GRANTED_RIGHTS: [&str; 1] = [INTERACTIVE_RIGHT];

/// The well-known holders that already admit the pool accounts: `Users`, `Everyone` and
/// `Authenticated Users` (the group's own SID is checked beside them).
pub const COVERING_HOLDERS: [&str; 3] = ["S-1-5-32-545", "S-1-1-0", "S-1-5-11"];

/// The LSA and the record's `LogonRightAdded`, as the grant needs them: the real calls on
/// Windows (`lsa_rights.rs`), a fake in the tests.
pub trait LogonRights {
    /// What policy says of `right` for `pool`, read afresh.
    fn policy(&mut self, right: &str, pool: Pool<'_>) -> RightPolicy;
    /// The SIDs local policy gives `right` to (`LsaEnumerateAccountsWithUserRight`).
    fn holders(&mut self, right: &str) -> Result<Vec<Sid>, MachineError>;
    /// Adds "Allow log on locally" to `sid` (`LsaAddAccountRights`); no other right.
    fn add_interactive(&mut self, sid: &Sid) -> Result<(), MachineError>;
    /// Takes "Allow log on locally" off `sid` (`LsaRemoveAccountRights`); not held is `Ok`.
    fn remove_interactive(&mut self, sid: &Sid) -> Result<(), MachineError>;
    /// The record's `LogonRightAdded`: the SID set-up added the right to, if it did.
    fn recorded(&mut self) -> Result<Option<Sid>, MachineError>;
    /// Writes `LogonRightAdded` as `sid`, or deletes it for `None`.
    fn record(&mut self, sid: Option<&Sid>) -> Result<(), MachineError>;
    /// Writes the record's `LogonPolicy` as `state`, found at `at` (seconds since 1970), for
    /// a launch's 1385 to read.
    fn record_policy(&mut self, state: PolicyState, at: u64) -> Result<(), MachineError>;
}

/// What the grant step did, or, from [`plan_grant`], will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Granted {
    /// Added to the group and recorded; from [`plan_grant`], to be added ([`add_recorded`]).
    Added,
    /// No policy defines the right and a local holder already admits the pool.
    Covered,
    /// Policy defines the right and includes the pool.
    PolicyIncludes,
    /// Policy couldn't be read: nothing was granted.
    Unknown,
}

/// Why the grant step stopped the set-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    /// Policy defines the right without the pool.
    Excluded,
    /// A call failed; nothing is left granted without its record.
    Failed(MachineError),
}

/// What the grant would do for the group `group` and the pool accounts `users`, as the module
/// says, changing nothing: [`Granted::Added`] means a grant is due.
pub fn plan_grant(
    rights: &mut dyn LogonRights,
    group: &Sid,
    users: &[Sid],
) -> Result<Granted, GrantError> {
    let pool = Pool {
        group: Some(group),
        users,
    };
    match rights.policy(INTERACTIVE_RIGHT, pool) {
        RightPolicy::Defined {
            includes_pool: true,
        } => return Ok(Granted::PolicyIncludes),
        RightPolicy::Defined {
            includes_pool: false,
        } => return Err(GrantError::Excluded),
        RightPolicy::Unknown => return Ok(Granted::Unknown),
        RightPolicy::NotDefined => {}
    }
    let holders = rights
        .holders(INTERACTIVE_RIGHT)
        .map_err(GrantError::Failed)?;
    if holders
        .iter()
        .any(|sid| sid == group || COVERING_HOLDERS.contains(&sid.as_str()))
    {
        return Ok(Granted::Covered);
    }
    Ok(Granted::Added)
}

/// The grant itself: `LogonRightAdded` written as `group` first, so no grant is ever left that
/// `--remove` can't find, then the right added; a failed add clears the value again. The
/// caller has written the record's other values already (`late_steps::logon_right`), so the
/// value never stands in a key no reader takes for a record.
pub fn add_recorded(rights: &mut dyn LogonRights, group: &Sid) -> Result<(), MachineError> {
    rights.record(Some(group))?;
    if let Err(error) = rights.add_interactive(group) {
        if let Err(clear) = rights.record(None) {
            tracing::warn!(%clear, "LogonRightAdded couldn't be cleared after a failed grant");
        }
        return Err(error);
    }
    Ok(())
}

/// [`plan_grant`], then [`add_recorded`] when a grant is due.
pub fn grant_interactive(
    rights: &mut dyn LogonRights,
    group: &Sid,
    users: &[Sid],
) -> Result<Granted, GrantError> {
    let planned = plan_grant(rights, group, users)?;
    if planned == Granted::Added {
        add_recorded(rights, group).map_err(GrantError::Failed)?;
    }
    Ok(planned)
}

/// Takes the right off `sid`, which this run added it to, and clears the record: a failed
/// set-up's rollback. What is left behind, in words, when either fails; then the caller keeps
/// the record, so `--remove` still finds the grant.
pub fn undo_grant(rights: &mut dyn LogonRights, sid: &Sid) -> Result<(), String> {
    rights
        .remove_interactive(sid)
        .and_then(|()| rights.record(None))
        .map_err(|error| format!("the \"Allow log on locally\" right set-up added: {error}"))
}

/// `--remove`'s step: the right taken off the SID `LogonRightAdded` names, then the value
/// cleared; whether it was. Only when that SID is `group`, the record's `ClauseoSandboxUsers`:
/// a value naming anything else is left alone (logged), and its SID keeps its rights. A right
/// set-up didn't add is never touched.
pub fn remove_grant(rights: &mut dyn LogonRights, group: &Sid) -> Result<bool, MachineError> {
    let Some(sid) = rights.recorded()? else {
        return Ok(false);
    };
    if sid != *group {
        tracing::warn!(
            %sid,
            "LogonRightAdded doesn't name the sandbox group; its right is left as it is"
        );
        return Ok(false);
    }
    rights.remove_interactive(&sid)?;
    rights.record(None)?;
    Ok(true)
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
