//! Pool accounts the machine's record doesn't list (0049, "Stranded pool accounts"): a record
//! whose `PoolSids` was blanked, cut short or corrupted, or a record lost whole while its group
//! and accounts stayed. Set-up (`stranded_setup.rs`) and `--remove` (`remove/stranded.rs`)
//! find them here, so none is left behind enabled and holding the logon right through
//! `ClauseoSandboxUsers`.
//!
//! The group counts only as this computer's own: a local group (an alias, `LookupAccountNameW`
//! on `<computer>\ClauseoSandboxUsers`) whose SID lies in this machine's account domain
//! (`S-1-5-21-<machine>-<rid>`, [`PoolAccounts::machine`]). A domain group of that name never
//! does: then no account is proven. An account the record doesn't list is proven Clauseo's only
//! when it is a member of that group (the one the record's `GroupSid` names, when the record
//! is trusted) and named as a pool user, and either:
//!
//! - it carries [`POOL_COMMENT`], the comment set-up gives every pool account it makes or keeps
//!   (`accounts::create_user`, `Machine::mark`); or
//! - it carries the older mark only (`stranded_os::marked`: a standard user with
//!   `UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE`, hidden from the sign-in screen by its
//!   `SpecialAccounts\UserList` value 0), no member of the group carries the comment, and the
//!   record's `MarkerVersion` is absent or older than [`MARKER_VERSION`]. So a pool set up
//!   since the comment never accepts an account with the older mark alone.
//!
//! A name alone never proves anything (D-B5: anyone who can make an account can make a
//! `ClauseoSb01`). A malformed `PoolSids` is never trusted for a SID: it reads as an empty pool.
//!
//! What each side does with them:
//!
//! - the record's `OwnerSid`, `GroupSid` and `DataDir` read: set-up adopts each proven account
//!   back into the record, keeping its SID (so the ledger's and the ACLs' references stay
//!   good), resetting its password as for its own accounts, then makes only what is missing;
//!   `--remove` removes each the way it removes a recorded one;
//! - no record, or one whose `OwnerSid`, `GroupSid` or `DataDir` can't be read: nothing is
//!   adopted (D5: no account, and none of the grants its SID holds on the last owner's matters,
//!   goes to whoever runs set-up next). Both take each proven account out as a recorded one goes
//!   (its profile, with its `ProfileList` entry; the account with its sign-in screen value; any
//!   user right its SID still holds; its scheduled tasks), then the group with its Remote
//!   Desktop deny once it is empty; set-up then makes a new pool.
//!
//! Every other account the survey sees, one condition short, is named in the log (warn, which
//! reaches the Event Log) and left as it is, with the group while it has members.

use std::fmt;

use crate::windows::pool::{POOL_GROUP, PoolUser, pool_number};
use crate::windows::sids::Sid;

/// The comment set-up gives every pool account (`usri1_comment`), the mark that proves it.
pub const POOL_COMMENT: &str = "Clauseo sandbox account; managed by Clauseo set-up";

/// The record's `MarkerVersion` once set-up marks its accounts with [`POOL_COMMENT`].
pub const MARKER_VERSION: u32 = 2;

/// One local account the survey looked at: in the group, or with a pool user's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// Its name, as the engine names pool users (`ClauseoSbNN`).
    pub name: String,
    /// Its SID.
    pub sid: Sid,
    /// Whether it is a member of the group found ([`PoolAccounts::group`]).
    pub in_group: bool,
    /// Whether its comment is exactly [`POOL_COMMENT`].
    pub commented: bool,
    /// Whether it carries the older mark: the flags and the sign-in screen value.
    pub marked: bool,
}

/// What `Machine::pool_accounts` (and `Removal::pool_accounts`) found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolAccounts {
    /// The SID of the local group `ClauseoSandboxUsers`, found by name, if it exists.
    pub group: Option<Sid>,
    /// This machine's account domain (`preflight::machine_domain`): the group counts only
    /// when its SID lies in it.
    pub machine: [u32; 3],
    /// Each local account in that group or with a pool user's name.
    pub seen: Vec<Seen>,
    /// Members of the group that aren't this computer's user accounts (a domain account, a
    /// group), each by `DOMAIN\name`, or `an account Windows can't name`: never proven,
    /// never removed.
    pub foreign: Vec<String>,
    /// The record's `MarkerVersion` ([`marker_version_of`]): `None` when absent; one that
    /// can't be read is `u32::MAX`, so the older mark proves nothing.
    pub marker_version: Option<u32>,
}

/// What a corrupt record still says (`Removal::read_partial`): each value that reads alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartialRecord {
    /// `OwnerSid`.
    pub owner: Option<Sid>,
    /// `GroupSid`.
    pub group: Option<Sid>,
    /// `DataDir`.
    pub data_dir: Option<std::path::PathBuf>,
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
    /// In the group with the older mark only, while the pool is marked with the comment.
    OlderMark,
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
            Why::OlderMark => {
                "is in ClauseoSandboxUsers without the comment Clauseo's set-up now gives"
            }
        };
        write!(f, "{} {why}", self.name)
    }
}

/// The survey sorted: the group when it is this computer's, the proven accounts, in
/// pool-number order, and the doubtful ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sorted {
    /// The local group, when its SID lies in this machine's account domain.
    pub group: Option<Sid>,
    /// Proven Clauseo's, as the module says.
    pub proven: Vec<PoolUser>,
    /// One condition short: named in the log and left.
    pub doubtful: Vec<Doubt>,
}

/// `accounts` sorted as the module says, leaving out every account whose SID `skip` lists (the
/// record's own users, and the owner or the user who asked). An account neither in the group
/// nor marked (a `ClauseoSb01` a user made, say) is neither: it isn't named. A group outside
/// this machine's account domain counts as none, and no account is in it.
pub fn sort(accounts: &PoolAccounts, skip: &[Sid]) -> Sorted {
    let group = accounts.group.clone().filter(|group| {
        let local = group.domain() == Some(accounts.machine);
        if !local {
            tracing::warn!("{POOL_GROUP} isn't this computer's own group; it is left alone");
        }
        local
    });
    let local = group.is_some();
    let commented_pool = accounts
        .seen
        .iter()
        .any(|s| local && s.in_group && s.commented);
    let older_allowed = !commented_pool
        && accounts
            .marker_version
            .is_none_or(|version| version < MARKER_VERSION);
    let mut sorted = Sorted {
        group,
        ..Sorted::default()
    };
    for account in accounts
        .seen
        .iter()
        .filter(|account| !skip.contains(&account.sid))
    {
        let in_group = local && account.in_group;
        let pool_name = pool_number(&account.name).is_some();
        let any_mark = account.commented || account.marked;
        let why = match (in_group, pool_name) {
            (true, true) if account.commented || (account.marked && older_allowed) => {
                sorted.proven.push(PoolUser {
                    name: account.name.clone(),
                    sid: account.sid.clone(),
                });
                continue;
            }
            (true, true) if account.marked => Why::OlderMark,
            (true, _) if !any_mark => Why::Unmarked,
            (true, _) => Why::NotPoolName,
            (false, true) if any_mark => Why::OutsideGroup,
            (false, _) => continue,
        };
        sorted.doubtful.push(Doubt {
            name: account.name.clone(),
            why,
        });
    }
    sorted.proven.sort_by_key(|user| pool_number(&user.name));
    sorted
}

/// The record's `MarkerVersion` as `sort` takes it, from its read: absent is `None`; one that
/// can't be read, or past `u32`, is `u32::MAX`, never absent, so an account with the older mark
/// alone is never proven on the strength of a value no one could read.
pub fn marker_version_of<E: fmt::Display>(read: Result<Option<u64>, E>) -> Option<u32> {
    match read {
        Ok(version) => version.map(|version| u32::try_from(version).unwrap_or(u32::MAX)),
        Err(error) => {
            tracing::warn!(%error, "the record's MarkerVersion can't be read");
            Some(u32::MAX)
        }
    }
}

/// An account's delete as `accounts::delete_user` makes it: its sign-in screen value off first
/// (`unhide`), so no value outlives the account to pre-mark a later one of its name; then the
/// account (`delete`); and when that fails, the value put back (`hide`), so the account keeps
/// its mark and the next run still proves it. The delete's error is the one given back.
pub fn delete_keeping_hide<E>(
    unhide: impl FnOnce() -> Result<(), E>,
    delete: impl FnOnce() -> Result<(), E>,
    hide: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    unhide()?;
    delete().inspect_err(|_| {
        if hide().is_err() {
            tracing::warn!("a sandbox user's sign-in screen value couldn't be put back");
        }
    })
}

/// Names each doubtful account, and each of the group's foreign members, in the log.
pub fn log_doubtful(doubtful: &[Doubt], foreign: &[String]) {
    for doubt in doubtful {
        tracing::warn!(account = %doubt.name, "{doubt}; it was left as it is");
    }
    for member in foreign {
        tracing::warn!(
            %member,
            "a member of {POOL_GROUP} isn't this computer's account; it was left"
        );
    }
}

/// The group's members that can't be proven Clauseo's, by name: the doubtful accounts in it,
/// then its foreign members. What an administrator takes out before the group can go.
pub fn unprovable_members(sorted: &Sorted, foreign: &[String]) -> Vec<String> {
    let in_group = sorted
        .doubtful
        .iter()
        .filter(|doubt| doubt.why != Why::OutsideGroup);
    in_group
        .map(|doubt| doubt.name.clone())
        .chain(foreign.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MACHINE: [u32; 3] = [5, 5, 5];

    fn seen(name: &str, rid: u32, in_group: bool, commented: bool, marked: bool) -> Seen {
        Seen {
            name: name.to_owned(),
            sid: Sid::parse(&format!("S-1-5-21-5-5-5-{rid}")).unwrap(),
            in_group,
            commented,
            marked,
        }
    }

    fn accounts(seen: Vec<Seen>, version: Option<u32>) -> PoolAccounts {
        PoolAccounts {
            group: Some(Sid::parse("S-1-5-21-5-5-5-1000").unwrap()),
            machine: MACHINE,
            seen,
            foreign: Vec::new(),
            marker_version: version,
        }
    }

    fn proven(sorted: &Sorted) -> Vec<&str> {
        sorted.proven.iter().map(|u| u.name.as_str()).collect()
    }

    fn doubtful(sorted: &Sorted) -> Vec<(&str, Why)> {
        sorted
            .doubtful
            .iter()
            .map(|d| (d.name.as_str(), d.why))
            .collect()
    }

    #[test]
    fn a_commented_member_with_a_pool_name_is_proven_whatever_its_flags() {
        let all = vec![
            seen("ClauseoSb03", 1003, true, true, false),
            seen("ClauseoSb01", 1001, true, true, true),
            seen("ClauseoSb02", 1002, true, false, false),
            seen("ClauseoSb04", 1004, false, true, true),
            seen("ClauseoSb05", 1005, false, false, false),
            seen("Helper", 1006, true, true, true),
        ];
        let sorted = sort(&accounts(all, Some(MARKER_VERSION)), &[]);
        assert_eq!(proven(&sorted), ["ClauseoSb01", "ClauseoSb03"]);
        assert_eq!(
            doubtful(&sorted),
            [
                ("ClauseoSb02", Why::Unmarked),
                ("ClauseoSb04", Why::OutsideGroup),
                ("Helper", Why::NotPoolName),
            ]
        );
    }

    #[test]
    fn the_older_mark_alone_is_proven_only_before_the_comment() {
        let older = || vec![seen("ClauseoSb01", 1001, true, false, true)];
        for version in [None, Some(1)] {
            let sorted = sort(&accounts(older(), version), &[]);
            assert_eq!(proven(&sorted), ["ClauseoSb01"], "{version:?}");
        }
        // The record says the pool is marked with the comment.
        let sorted = sort(&accounts(older(), Some(MARKER_VERSION)), &[]);
        assert_eq!(doubtful(&sorted), [("ClauseoSb01", Why::OlderMark)]);
        assert_eq!(proven(&sorted), Vec::<&str>::new());
        // Another member carries the comment, a recorded one (skipped) included.
        let mut mixed = older();
        mixed.push(seen("ClauseoSb02", 1002, true, true, true));
        let skip = [mixed[1].sid.clone()];
        let sorted = sort(&accounts(mixed, None), &skip);
        assert_eq!(doubtful(&sorted), [("ClauseoSb01", Why::OlderMark)]);
        assert_eq!(proven(&sorted), Vec::<&str>::new());
        // Neither mark at all is never proven.
        let bare = vec![seen("ClauseoSb01", 1001, true, false, false)];
        assert_eq!(
            doubtful(&sort(&accounts(bare, None), &[])),
            [("ClauseoSb01", Why::Unmarked)]
        );
    }

    #[test]
    fn an_unreadable_marker_version_never_lets_the_older_mark_prove() {
        assert_eq!(marker_version_of::<String>(Ok(None)), None);
        assert_eq!(marker_version_of::<String>(Ok(Some(2))), Some(2));
        assert_eq!(
            marker_version_of::<String>(Ok(Some(u64::MAX))),
            Some(u32::MAX)
        );
        let unreadable = marker_version_of(Err("the value has the wrong type"));
        assert_eq!(unreadable, Some(u32::MAX));
        let older = vec![seen("ClauseoSb01", 1001, true, false, true)];
        let sorted = sort(&accounts(older, unreadable), &[]);
        assert_eq!(doubtful(&sorted), [("ClauseoSb01", Why::OlderMark)]);
    }

    #[test]
    fn a_failed_delete_puts_the_sign_in_value_back() {
        let steps = std::cell::RefCell::new(Vec::new());
        let step = |name: &'static str, ok: bool| {
            let steps = &steps;
            move || {
                steps.borrow_mut().push(name);
                if ok { Ok(()) } else { Err(name) }
            }
        };
        let failed = delete_keeping_hide(
            step("unhide", true),
            step("delete", false),
            step("hide", true),
        );
        assert_eq!(failed, Err("delete"));
        assert_eq!(*steps.borrow(), ["unhide", "delete", "hide"]);
        steps.borrow_mut().clear();
        let done = delete_keeping_hide(
            step("unhide", true),
            step("delete", true),
            step("hide", true),
        );
        assert_eq!(done, Ok(()));
        assert_eq!(*steps.borrow(), ["unhide", "delete"]);
    }

    #[test]
    fn a_domain_group_of_that_name_proves_nothing() {
        let mut found = accounts(vec![seen("ClauseoSb01", 1001, true, true, true)], None);
        found.group = Some(Sid::parse("S-1-5-21-9-9-9-1000").unwrap());
        let sorted = sort(&found, &[]);
        assert_eq!(sorted.group, None);
        assert_eq!(proven(&sorted), Vec::<&str>::new());
        assert_eq!(doubtful(&sorted), [("ClauseoSb01", Why::OutsideGroup)]);
        // A well-known group (`BUILTIN\...`) isn't in the account domain either.
        found.group = Some(Sid::parse("S-1-5-32-545").unwrap());
        assert_eq!(sort(&found, &[]).group, None);
    }

    #[test]
    fn a_skipped_sid_is_neither_proven_nor_named() {
        let all = vec![seen("ClauseoSb01", 1001, true, true, true)];
        let skip = [all[0].sid.clone()];
        let sorted = sort(&accounts(all, None), &skip);
        assert_eq!((sorted.proven.len(), sorted.doubtful.len()), (0, 0));
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
