//! Which running processes set-up and removal stop before deleting a pool user's profile
//! (D-B13): a loaded hive makes `DeleteProfileW` fail, so a process still running as a pool
//! user (a stray, since the app is closed by then) is ended first. Pure, so the rule is tested on
//! every OS; the process list and the termination are in `processes.rs` (Windows only).
//!
//! The rule: a process is stopped only when its token user is, exactly, one of the pool's SIDs.
//! A process whose user couldn't be read is never stopped, and neither is this process or the
//! two the kernel runs (pid 0, the idle process, and pid 4, System).

use crate::windows::pool::pool_number;
use crate::windows::sids::Sid;

/// The idle process's pid.
const IDLE_PID: u32 = 0;
/// The System process's pid.
const SYSTEM_PID: u32 = 4;

/// The lowest relative id a pool account can have: Windows gives local accounts 1000 and up,
/// and keeps the ones below for built-in accounts and groups (500 is Administrator).
pub const POOL_RID_MIN: u32 = 1000;

/// Why a SID is refused as a pool user's, before any process is stopped or profile deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotPool {
    /// Not an account of this machine: a well-known SID, another machine's, or a domain's.
    #[error("it isn't an account of this computer")]
    NotThisMachine,
    /// An account of this machine with a relative id under [`POOL_RID_MIN`]: a built-in one.
    #[error("it is a built-in account of this computer")]
    BuiltIn,
    /// The user this process runs as.
    #[error("it is the user running set-up")]
    ThisUser,
    /// The owner: the user who asked for the set-up, who may not be the user running it.
    #[error("it is the owner of the sandbox")]
    Owner,
    /// It names an account whose name isn't a pool user's (`ClauseoSbNN`).
    #[error("its account isn't a pool user")]
    OtherName,
}

/// The account name prefix of the elevated logon tests (`ClauseoSbTL` and six hex digits),
/// which the check takes as a pool user's in test builds only.
#[cfg(test)]
pub(crate) const TEST_PREFIX: &str = "ClauseoSbTL";

/// The account name prefix of the setup exe's elevated test (`PR4h`): `ClauseoSbTS`, four hex
/// digits for the run, then the pool user's two digits; a pool user's in test builds only.
#[cfg(test)]
pub(crate) const SETUP_TEST_PREFIX: &str = "ClauseoSbTS";

/// Whether `name` is a pool user's account name: `ClauseoSbNN` exactly ([`pool_number`]),
/// or in test builds a [`TEST_PREFIX`] or [`SETUP_TEST_PREFIX`] name.
pub fn is_pool_account_name(name: &str) -> bool {
    if pool_number(name).is_some() {
        return true;
    }
    #[cfg(test)]
    if let Some(suffix) = name.strip_prefix(TEST_PREFIX) {
        return suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    #[cfg(test)]
    if let Some(suffix) = name.strip_prefix(SETUP_TEST_PREFIX) {
        let (run, user) = suffix.split_at_checked(4).unwrap_or_default();
        return run.bytes().all(|byte| byte.is_ascii_hexdigit())
            && run.len() == 4
            && pool_number(&format!("{}{user}", crate::windows::pool::POOL_PREFIX)).is_some();
    }
    false
}

/// Checks that `sid` can be a pool user's, as defence in depth before a stop or a delete: a
/// local account (`S-1-5-21-<machine>-<rid>`) in `machine`, this machine's account domain,
/// with a relative id of at least [`POOL_RID_MIN`]; not `own`, this process's user, nor
/// `owner`, the user who asked (in an over-the-shoulder run they differ); and, when it
/// resolves, an account named as a pool user ([`is_pool_account_name`]). `name` is the
/// account `sid` resolves to, `None` when it resolves to none: a deleted pool account may
/// still own a profile or a process, and the owner's account always resolves.
pub fn check_pool_sid(
    sid: &Sid,
    machine: [u32; 3],
    own: &Sid,
    owner: &Sid,
    name: Option<&str>,
) -> Result<(), NotPool> {
    let parts: Vec<&str> = sid.as_str().split('-').collect();
    // S, 1, 5, 21, the domain's three parts, the relative id.
    if parts.len() != 8 || sid.domain() != Some(machine) {
        return Err(NotPool::NotThisMachine);
    }
    let rid = parts
        .last()
        .and_then(|rid| rid.parse::<u32>().ok())
        .ok_or(NotPool::NotThisMachine)?;
    if rid < POOL_RID_MIN {
        return Err(NotPool::BuiltIn);
    }
    if sid == own {
        return Err(NotPool::ThisUser);
    }
    if sid == owner {
        return Err(NotPool::Owner);
    }
    if name.is_some_and(|name| !is_pool_account_name(name)) {
        return Err(NotPool::OtherName);
    }
    Ok(())
}

/// One running process as the list saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// Its process id.
    pub pid: u32,
    /// Its token's user; `None` when the token couldn't be read.
    pub user: Option<Sid>,
}

/// The pids of `seen` to stop: each process whose token user is one of `pool`, compared as
/// canonical SIDs ([`Sid`]'s equality is exact), skipping `own_pid`, pid 0 and pid 4. In the
/// order seen, each pid once.
pub fn to_stop(seen: &[Seen], pool: &[Sid], own_pid: u32) -> Vec<u32> {
    let mut stop: Vec<u32> = Vec::new();
    for process in seen {
        if [own_pid, IDLE_PID, SYSTEM_PID].contains(&process.pid) || stop.contains(&process.pid) {
            continue;
        }
        let Some(user) = &process.user else {
            continue;
        };
        if pool.iter().any(|pool_sid| pool_sid == user) {
            stop.push(process.pid);
        }
    }
    stop
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOL_1: &str = "S-1-5-21-111-222-333-1001";
    const POOL_2: &str = "S-1-5-21-111-222-333-1002";
    const OWNER: &str = "S-1-5-21-111-222-333-1000";

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    fn seen(pid: u32, user: Option<&str>) -> Seen {
        Seen {
            pid,
            user: user.map(sid),
        }
    }

    fn pool() -> Vec<Sid> {
        vec![sid(POOL_1), sid(POOL_2)]
    }

    const MACHINE: [u32; 3] = [111, 222, 333];

    /// The administrator running an over-the-shoulder set-up: not the owner.
    const ADMIN: &str = "S-1-5-21-111-222-333-1003";

    #[test]
    fn only_this_machines_non_built_in_accounts_other_than_this_user_pass() {
        let (admin, owner) = (sid(ADMIN), sid(OWNER));
        let check = |text: &str| check_pool_sid(&sid(text), MACHINE, &admin, &owner, None);
        assert_eq!(check(POOL_1), Ok(()));
        assert_eq!(check(ADMIN), Err(NotPool::ThisUser));
        // Well-known SIDs: SYSTEM, Administrators, Users, Everyone.
        for text in ["S-1-5-18", "S-1-5-32-544", "S-1-5-32-545", "S-1-1-0"] {
            assert_eq!(check(text), Err(NotPool::NotThisMachine), "{text}");
        }
        // A domain's account, another machine's, and the machine's domain SID itself.
        assert_eq!(check("S-1-5-21-7-8-9-1105"), Err(NotPool::NotThisMachine));
        assert_eq!(
            check("S-1-5-21-111-222-334-1001"),
            Err(NotPool::NotThisMachine)
        );
        assert_eq!(check("S-1-5-21-111-222-333"), Err(NotPool::NotThisMachine));
        assert_eq!(
            check("S-1-5-21-111-222-333-5-1001"),
            Err(NotPool::NotThisMachine)
        );
        // Built-in accounts: Administrator (500), Guest (501), DefaultAccount (503), and 999.
        for rid in [500, 501, 503, 999] {
            let text = format!("S-1-5-21-111-222-333-{rid}");
            assert_eq!(check(&text), Err(NotPool::BuiltIn), "{text}");
        }
    }

    #[test]
    fn the_owner_is_refused_when_another_user_runs_set_up() {
        let (admin, owner) = (sid(ADMIN), sid(OWNER));
        let named = Some("ClauseoSb01");
        assert_eq!(
            check_pool_sid(&owner, MACHINE, &admin, &owner, named),
            Err(NotPool::Owner)
        );
        assert_eq!(
            check_pool_sid(&owner, MACHINE, &admin, &owner, None),
            Err(NotPool::Owner)
        );
    }

    #[test]
    fn a_sid_resolving_to_a_non_pool_name_is_refused_and_one_resolving_to_none_passes() {
        let (admin, owner, pool) = (sid(ADMIN), sid(OWNER), sid(POOL_1));
        let check = |name: Option<&str>| check_pool_sid(&pool, MACHINE, &admin, &owner, name);
        assert_eq!(check(Some("ClauseoSb01")), Ok(()));
        assert_eq!(check(Some("ClauseoSb32")), Ok(()));
        assert_eq!(check(None), Ok(()));
        // The test family's names pass in test builds only.
        assert_eq!(check(Some("ClauseoSbTL0a1b2c")), Ok(()));
        for name in [
            "alice",
            "Administrator",
            "ClauseoSb00",
            "ClauseoSb33",
            "ClauseoSb1",
            "clauseosb01",
            "ClauseoSb01x",
            "ClauseoSbTest01",
            "ClauseoSbTL0a1b2",
            "ClauseoSbTLzzzzzz",
        ] {
            assert_eq!(check(Some(name)), Err(NotPool::OtherName), "{name}");
        }
    }

    #[test]
    fn only_processes_running_as_a_pool_sid_are_stopped() {
        let list = [
            seen(100, Some(POOL_1)),
            seen(101, Some(OWNER)),
            seen(102, Some("S-1-5-18")),
            seen(103, Some(POOL_2)),
            seen(104, Some("S-1-5-32-544")),
        ];
        assert_eq!(to_stop(&list, &pool(), 999), [100, 103]);
    }

    #[test]
    fn sids_are_compared_exactly_never_by_prefix_or_domain() {
        let list = [
            // The pool user's SID with a digit more, and with a digit less.
            seen(200, Some("S-1-5-21-111-222-333-10011")),
            seen(201, Some("S-1-5-21-111-222-333-100")),
            // The same RID in another domain.
            seen(202, Some("S-1-5-21-111-222-334-1001")),
            // The machine's domain itself.
            seen(203, Some("S-1-5-21-111-222-333")),
            seen(204, Some(POOL_1)),
        ];
        assert_eq!(to_stop(&list, &pool(), 999), [204]);
    }

    #[test]
    fn an_unread_user_is_never_stopped() {
        let list = [seen(300, None), seen(301, None)];
        assert_eq!(to_stop(&list, &pool(), 999), Vec::<u32>::new());
    }

    #[test]
    fn the_setup_tests_names_pass_only_in_their_exact_form() {
        assert!(is_pool_account_name("ClauseoSbTSa1f001"));
        assert!(is_pool_account_name("ClauseoSbTS00ff32"));
        for other in [
            "ClauseoSbTSa1f000",
            "ClauseoSbTSa1f033",
            "ClauseoSbTSa1f01",
            "ClauseoSbTSa1g001",
            "ClauseoSbTS",
            "ClauseoSbTSa1f0011",
        ] {
            assert!(!is_pool_account_name(other), "{other}");
        }
    }

    #[test]
    fn an_empty_pool_stops_nothing() {
        let list = [seen(400, Some(POOL_1)), seen(401, Some(OWNER))];
        assert_eq!(to_stop(&list, &[], 999), Vec::<u32>::new());
    }

    #[test]
    fn this_process_and_the_kernels_are_never_stopped() {
        let list = [
            seen(0, Some(POOL_1)),
            seen(4, Some(POOL_1)),
            seen(999, Some(POOL_1)),
            seen(500, Some(POOL_1)),
        ];
        assert_eq!(to_stop(&list, &pool(), 999), [500]);
    }

    #[test]
    fn a_pid_listed_twice_is_stopped_once() {
        let list = [seen(600, Some(POOL_2)), seen(600, Some(POOL_2))];
        assert_eq!(to_stop(&list, &pool(), 999), [600]);
    }
}
