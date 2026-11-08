//! The browser's gate on a slot's pool and daemon ports (0049, "Loopback ports per slot"): a
//! check, not the boundary. Set-up's filters are the boundary; the gate catches what they
//! allow and a run must not use:
//!
//! - **at launch** ([`idle_at_launch`]), no TCP listener on any of the slot's pool or daemon
//!   ports. A browser or daemon an earlier run of the same slot left would run as the same pool
//!   user, so the filters let the new run reach it, with another matter's pages and cookies. A
//!   lawyer's program that Windows gave a daemon port is just as reachable. Either way the
//!   browser is off for the run;
//! - **after start**, and again whenever agent-browser's stream server falls back to another
//!   port ([`in_job_gate`]), every listener on those ports must be a process in this launch's
//!   Job Object; else the browser is off. A fallback port outside the slot is one the agent
//!   can't reach, so it fails closed on its own.
//!
//! The rules are plain data over (port, process id) rows; [`check_browser_ports`] and
//! [`check_browser_job`] read Windows' TCP table, and ask Windows whether each process is in the
//! job (`OpenProcess` with `PROCESS_QUERY_LIMITED_INFORMATION`, then `IsProcessInJob`). A process
//! that can't be opened counts as outside the job.

use super::Slot;

/// Why the browser is off for one run, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the browser is off for this run: {0}")]
pub struct BrowserOff(pub String);

/// The listeners in `rows` (port, process id) on `slot`'s pool or daemon ports.
fn on_slot(slot: Slot, rows: &[(u16, u32)]) -> impl Iterator<Item = &(u16, u32)> {
    let (pool, daemon) = (slot.pool(), slot.daemon());
    rows.iter()
        .filter(move |(port, _)| pool.contains(port) || daemon.contains(port))
}

/// Checks that no listener in `rows` (port, process id), any address, IPv4 or IPv6, is on one of
/// `slot`'s pool or daemon ports, before the browser's ports are claimed.
pub fn idle_at_launch(slot: Slot, rows: &[(u16, u32)]) -> Result<(), BrowserOff> {
    match on_slot(slot, rows).next() {
        None => Ok(()),
        Some((port, pid)) => Err(BrowserOff(format!(
            "port {port}, one of this matter's browser ports, is already in use (process \
             {pid}), perhaps by a browser an earlier session left running; the browser comes \
             back once that program has closed"
        ))),
    }
}

/// Checks that every listener in `rows` (port, process id) on `slot`'s pool or daemon ports is
/// a process `in_job` says is in this launch's job (`None`: it couldn't be asked, so outside).
pub fn in_job_gate(
    slot: Slot,
    rows: &[(u16, u32)],
    mut in_job: impl FnMut(u32) -> Option<bool>,
) -> Result<(), BrowserOff> {
    match on_slot(slot, rows).find(|(_, pid)| in_job(*pid) != Some(true)) {
        None => Ok(()),
        Some((port, pid)) => Err(BrowserOff(format!(
            "port {port}, one of this matter's browser ports, is held by a program outside \
             this session (process {pid})"
        ))),
    }
}

/// [`idle_at_launch`] on Windows' TCP listeners now. A table that can't be read turns the
/// browser off.
#[cfg(windows)]
pub fn check_browser_ports(slot: Slot) -> Result<(), BrowserOff> {
    idle_at_launch(slot, &listener_rows()?)
}

/// [`in_job_gate`] on Windows' TCP listeners now, against `job`, the launch's Job Object
/// (which needs `JOB_OBJECT_QUERY`). Call it once the browser's listeners are up, and again
/// when the stream server falls back to another port.
#[cfg(windows)]
pub fn check_browser_job(
    slot: Slot,
    job: std::os::windows::io::BorrowedHandle<'_>,
) -> Result<(), BrowserOff> {
    use std::os::windows::io::AsRawHandle;
    let job = job.as_raw_handle();
    in_job_gate(slot, &listener_rows()?, |pid| {
        crate::windows::wfp::process_in_job(pid, job)
    })
}

/// Windows' TCP listeners as (port, process id) rows.
#[cfg(windows)]
fn listener_rows() -> Result<Vec<(u16, u32)>, BrowserOff> {
    crate::windows::wfp::listener_rows()
        .map_err(|gap| BrowserOff(format!("its ports couldn't be checked ({gap})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(n: u8) -> Slot {
        Slot::new(n).unwrap()
    }

    #[test]
    fn a_listener_on_the_slots_pool_or_daemon_ports_turns_the_browser_off_at_launch() {
        let s = slot(3);
        let other = slot(4);
        let fine = [
            (*s.host().start(), 1),
            (*other.pool().start(), 2),
            (*other.daemon().end(), 3),
            (8080, 4),
        ];
        assert_eq!(idle_at_launch(s, &fine), Ok(()));
        for port in [*s.pool().start(), *s.pool().end(), *s.daemon().start()] {
            let off = idle_at_launch(s, &[(8080, 4), (port, 77)]).unwrap_err();
            let text = off.to_string();
            assert!(
                text.starts_with("the browser is off for this run"),
                "{text}"
            );
            assert!(
                text.contains(&port.to_string()) && text.contains("77"),
                "{text}"
            );
        }
    }

    #[test]
    fn after_start_every_slot_listener_must_be_in_the_job_of_the_launch() {
        let s = slot(0);
        let (cdp, daemon) = (*s.pool().start(), *s.daemon().start());
        let job = |pid: u32| match pid {
            10 | 11 => Some(true),
            20 => Some(false),
            _ => None,
        };
        let ours = [(cdp, 10), (daemon, 11), (*s.host().start(), 99), (9000, 20)];
        assert_eq!(in_job_gate(s, &ours, job), Ok(()));
        let stale = [(cdp, 10), (daemon, 20)];
        let off = in_job_gate(s, &stale, job).unwrap_err();
        assert!(off.0.contains(&daemon.to_string()), "{off}");
        let unknown = [(cdp + 1, 30)];
        assert!(
            in_job_gate(s, &unknown, job).is_err(),
            "a process it can't ask is outside"
        );
        // A stream server that fell back into the slot is checked again.
        let fallback = [(cdp, 10), (cdp + 2, 20)];
        assert!(in_job_gate(s, &fallback, job).is_err());
    }
}
