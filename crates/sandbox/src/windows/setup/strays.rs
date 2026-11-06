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
