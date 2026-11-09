//! Pool accounts the machine's record doesn't list (0049, "Stranded pool accounts"): a record
//! whose `PoolSids` was blanked, cut short or corrupted, or a record lost whole while its group
//! and accounts stayed. Set-up and `--remove` find them here, so none is left behind enabled
//! and holding the logon right through `ClauseoSandboxUsers`.
//!
//! An account is proven Clauseo's only when both hold:
//!
//! - it is a member of `ClauseoSandboxUsers`: the group whose SID the record's `GroupSid` names,
//!   or, with no record to trust, the group by that name, which only an administrator can make;
//! - it carries set-up's mark (`stranded_os::marked`): the account `NetUserAdd` made for a
//!   pool user, a standard user (`USER_PRIV_USER`) with `UF_DONT_EXPIRE_PASSWD |
//!   UF_PASSWD_CANT_CHANGE` (`accounts::create_user`), and hidden from the sign-in screen by
//!   its `SpecialAccounts\UserList` value 0 (`accounts::hide`).
//!
//! It must also have a pool user's name, as every account set-up makes does, but a name alone
//! never proves anything (D-B5: anyone who can make an account can make a `ClauseoSb01`). A
//! malformed `PoolSids` is never trusted for a SID: it reads as an empty pool.
//!
//! What each side does with them:
//!
//! - the record's `OwnerSid`, `GroupSid` and `DataDir` read: set-up adopts each proven account
//!   back into the record, keeping its SID (so the ledger's and the ACLs' references stay
//!   good), resetting its password as for its own accounts, then makes only what is missing;
//!   `--remove` removes each the way it removes a recorded one (`remove/stranded.rs`);
//! - no record, or one whose `OwnerSid`, `GroupSid` or `DataDir` can't be read: nothing is
//!   adopted (D5: no account, and none of the grants its SID holds on the last owner's matters,
//!   goes to whoever runs set-up next). Both take each proven account out with its profile,
//!   then the group with its Remote Desktop deny once it is empty; set-up then makes a new pool.
//!
//! Every other account the survey sees, one condition short, is named in the log (warn, which
//! reaches the Event Log) and left as it is, with the group while it has members.

use std::fmt;

use super::machine::{Machine, MachineError};
use super::outcome::{SetupFailure, Step, cause_of};
use crate::windows::pool::{MachineRecord, POOL_GROUP, PoolUser, pool_number};
use crate::windows::sids::Sid;

/// One local account the survey looked at: in the group, or with a pool user's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// Its name, as the engine names pool users (`ClauseoSbNN`).
    pub name: String,
    /// Its SID.
    pub sid: Sid,
    /// Whether it is a member of the group found ([`PoolAccounts::group`]).
    pub in_group: bool,
    /// Whether it carries set-up's mark.
    pub marked: bool,
}

/// What `Machine::pool_accounts` (and `Removal::pool_accounts`) found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolAccounts {
    /// The SID of the local group `ClauseoSandboxUsers`, found by name, if it exists.
    pub group: Option<Sid>,
    /// Each local account in that group or with a pool user's name.
    pub seen: Vec<Seen>,
    /// Members of the group that aren't this computer's user accounts (a domain account, a
    /// group): never proven, never removed.
    pub foreign: usize,
}

/// Why an account isn't proven Clauseo's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// In the group, without set-up's mark.
    Unmarked,
    /// Marked and named as a pool user, but not in the group.
    OutsideGroup,
    /// In the group and marked, but not named as a pool user.
    NotPoolName,
}

/// An account one condition short: named, never touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doubt {
    /// The account's name.
    pub name: String,
    /// What it lacks.
    pub why: Why,
}

impl fmt::Display for Doubt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self.why {
            Why::Unmarked => "is in ClauseoSandboxUsers but wasn't made by Clauseo's set-up",
            Why::OutsideGroup => {
                "looks like a Clauseo sandbox user but isn't in ClauseoSandboxUsers"
            }
            Why::NotPoolName => "is in ClauseoSandboxUsers but isn't named as a sandbox user",
        };
        write!(f, "{} {why}", self.name)
    }
}

/// The survey sorted: the proven accounts, in pool-number order, and the doubtful ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sorted {
    /// In the group, marked and named as pool users: Clauseo's.
    pub proven: Vec<PoolUser>,
    /// One condition short: named in the log and left.
    pub doubtful: Vec<Doubt>,
}

/// `seen` sorted as the module says, leaving out every account whose SID `skip` lists (the
/// record's own users, and the owner or the user who asked). An account neither in the group
/// nor marked (a `ClauseoSb01` a user made, say) is neither: it isn't named.
pub fn sort(seen: &[Seen], skip: &[Sid]) -> Sorted {
    let mut sorted = Sorted::default();
    for account in seen.iter().filter(|account| !skip.contains(&account.sid)) {
        let pool_name = pool_number(&account.name).is_some();
        let why = match (account.in_group, account.marked, pool_name) {
            (true, true, true) => {
                sorted.proven.push(PoolUser {
                    name: account.name.clone(),
                    sid: account.sid.clone(),
                });
                continue;
            }
            (true, false, _) => Why::Unmarked,
            (true, true, false) => Why::NotPoolName,
            (false, true, true) => Why::OutsideGroup,
            (false, _, _) => continue,
        };
        sorted.doubtful.push(Doubt {
            name: account.name.clone(),
            why,
        });
    }
    sorted.proven.sort_by_key(|user| pool_number(&user.name));
    sorted
}

/// Names each doubtful account, and counts the group's foreign members, in the log.
pub fn log_doubtful(doubtful: &[Doubt], foreign: usize) {
    for doubt in doubtful {
        tracing::warn!(account = %doubt.name, "{doubt}; it was left as it is");
    }
    if foreign > 0 {
        tracing::warn!(
            foreign,
            "{POOL_GROUP} has members that aren't this computer's accounts; they were left"
        );
    }
}

/// The record set-up starts from: the record when it can be trusted; otherwise, with
/// everything a lost or corrupt record stranded taken out first, none. A group left with
/// members set-up can't prove are Clauseo's refuses the run, naming them.
pub(super) fn record_or_sweep(
    machine: &mut impl Machine,
) -> Result<Option<MachineRecord>, SetupFailure> {
    match machine.read_record() {
        Ok(Some(record)) => return Ok(Some(record)),
        Ok(None) => {}
        Err(MachineError::RecordCorrupt(why)) => tracing::warn!(
            %why,
            "the machine's record can't be trusted; set-up removes the sandbox users it can \
             prove are Clauseo's and makes a new pool"
        ),
        Err(error) => return Err(SetupFailure::early(Step::ReadRecord, cause_of(&error))),
    }
    let accounts = machine
        .pool_accounts()
        .map_err(|error| SetupFailure::early(Step::Pool, cause_of(&error)))?;
    let skip = [machine.owner().clone()];
    let sorted = sort(&accounts.seen, &skip);
    log_doubtful(&sorted.doubtful, accounts.foreign);
    let Some(group) = accounts.group else {
        return Ok(None);
    };
    let failed = |step: Step, error: &MachineError| SetupFailure::early(step, cause_of(error));
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user no record lists is removed");
        let profile = machine
            .profile_of(&user.sid)
            .map_err(|error| failed(Step::Profile, &error))?;
        if profile.is_some() {
            machine
                .delete_profile(&user.sid)
                .map_err(|error| failed(Step::Profile, &error))?;
        }
        machine
            .delete_user(&user.name)
            .map_err(|error| failed(Step::CreateUser, &error))?;
    }
    let left = machine
        .group_members(&group)
        .map_err(|error| failed(Step::Group, &error))?;
    if left > 0 {
        let names: Vec<&str> = sorted.doubtful.iter().map(|d| d.name.as_str()).collect();
        return Err(SetupFailure::early(
            Step::Group,
            format!(
                "the group {POOL_GROUP}, which no record names, still has {left} member(s) \
                 set-up can't prove are Clauseo's ({}); take them out of the group, then run \
                 set-up again",
                names.join(", ")
            ),
        ));
    }
    machine
        .remove_remote_logon_deny(&group)
        .map_err(|error| failed(Step::RemoteLogon, &error))?;
    machine
        .delete_group(&group)
        .map_err(|error| failed(Step::Group, &error))?;
    Ok(None)
}

/// The proven accounts `record` doesn't list, for set-up to adopt: none when the group by name
/// isn't the record's (the group step then refuses).
pub(super) fn adoptable(
    machine: &mut impl Machine,
    record: &MachineRecord,
) -> Result<Vec<PoolUser>, SetupFailure> {
    let accounts = machine
        .pool_accounts()
        .map_err(|error| SetupFailure::early(Step::Pool, cause_of(&error)))?;
    let mut skip = record.pool.clone();
    skip.push(record.owner.clone());
    let mut sorted = sort(&accounts.seen, &skip);
    if accounts.group.as_ref() != Some(&record.group) {
        sorted.proven.clear();
    }
    log_doubtful(&sorted.doubtful, accounts.foreign);
    for user in &sorted.proven {
        tracing::warn!(account = %user.name, "a sandbox user the record lost is adopted back");
    }
    Ok(sorted.proven)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(name: &str, rid: u32, in_group: bool, marked: bool) -> Seen {
        Seen {
            name: name.to_owned(),
            sid: Sid::parse(&format!("S-1-5-21-5-5-5-{rid}")).unwrap(),
            in_group,
            marked,
        }
    }

    #[test]
    fn only_a_marked_member_with_a_pool_name_is_proven() {
        let all = [
            seen("ClauseoSb03", 1003, true, true),
            seen("ClauseoSb01", 1001, true, true),
            seen("ClauseoSb02", 1002, true, false),
            seen("ClauseoSb04", 1004, false, true),
            seen("ClauseoSb05", 1005, false, false),
            seen("Helper", 1006, true, true),
        ];
        let sorted = sort(&all, &[]);
        let proven: Vec<&str> = sorted.proven.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(proven, ["ClauseoSb01", "ClauseoSb03"]);
        let doubtful: Vec<(&str, Why)> = sorted
            .doubtful
            .iter()
            .map(|d| (d.name.as_str(), d.why))
            .collect();
        assert_eq!(
            doubtful,
            [
                ("ClauseoSb02", Why::Unmarked),
                ("ClauseoSb04", Why::OutsideGroup),
                ("Helper", Why::NotPoolName),
            ]
        );
    }

    #[test]
    fn a_skipped_sid_is_neither_proven_nor_named() {
        let all = [seen("ClauseoSb01", 1001, true, true)];
        let skip = [all[0].sid.clone()];
        assert_eq!(sort(&all, &skip), Sorted::default());
    }

    #[test]
    fn a_doubt_names_the_account_and_never_a_sid() {
        let doubt = Doubt {
            name: "ClauseoSb02".to_owned(),
            why: Why::Unmarked,
        };
        let text = doubt.to_string();
        assert!(text.starts_with("ClauseoSb02 "), "{text}");
        assert!(!text.contains("S-1-5"), "{text}");
    }
}
