//! Which running processes set-up and removal stop before deleting a pool user's profile
//! (D-B13): a loaded hive makes `DeleteProfileW` fail, so a process still running as a pool
//! user (a stray, since the app is closed by then) is ended first. Pure, so the rule is tested on
//! every OS; the process list and the termination are in `processes.rs` (Windows only).
//!
//! The rule: a process is stopped only when its token user is, exactly, one of the pool's SIDs.
//! A process whose user couldn't be read is never stopped, and neither is this process or the
//! two the kernel runs (pid 0, the idle process, and pid 4, System).

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
}

/// Checks that `sid` can be a pool user's, as defence in depth before a stop or a delete: a
/// local account (`S-1-5-21-<machine>-<rid>`) in `machine`, this machine's account domain,
/// with a relative id of at least [`POOL_RID_MIN`], and not `own`, this process's user.
pub fn check_pool_sid(sid: &Sid, machine: [u32; 3], own: &Sid) -> Result<(), NotPool> {
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

    #[test]
    fn only_this_machines_non_built_in_accounts_other_than_this_user_pass() {
        let own = sid(OWNER);
        let check = |text: &str| check_pool_sid(&sid(text), MACHINE, &own);
        assert_eq!(check(POOL_1), Ok(()));
        assert_eq!(check("S-1-5-21-111-222-333-1000"), Err(NotPool::ThisUser));
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
