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
//!   Job Object, and each port the launch expects (Chrome's CDP port, the stream server's, the
//!   daemon's) must have one; else the browser is off. A daemon or stream server that fell back
//!   to a port outside the slot (Windows handed its preferred port out, say) leaves its expected
//!   port with no listener, so the run gets a plain "browser off" line, not a browser the agent
//!   can't reach.
//!
//! The rules are plain data over (port, process id) rows; [`check_browser_ports`] and
//! [`check_browser_job`] read Windows' TCP table. Whether a listener's process is in the job is
//! asked through a handle opened first ([`listener_in_job`]): `OpenProcess` with
//! `PROCESS_QUERY_LIMITED_INFORMATION`, then the TCP table read again to confirm the port is
//! still that pid's (an open handle keeps Windows from giving the pid to another process) and
//! that the process was created before the socket was bound, then `IsProcessInJob` on that
//! handle. A process that can't be opened, a row that changed, or a process younger than its
//! socket counts as outside the job.
//!
//! None of this is wired yet: the host's `BrowserSession::claim` change and PR6 call it.

use super::Slot;

/// Why the browser is off for one run, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the browser is off for this run: {0}")]
pub struct BrowserOff(pub String);

/// A TCP listener as the gate reads it again: its port, its process, and when its socket was
/// bound (`FILETIME` ticks; 0 when Windows doesn't say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundListener {
    /// Its local port.
    pub port: u16,
    /// Its process id.
    pub pid: u32,
    /// When its socket was bound, or 0.
    pub bound_at: u64,
}

/// What the gate asks Windows about a listener's process, through a handle it holds.
pub trait JobProbe {
    /// An open handle to a process.
    type Process;
    /// Opens process `pid`; `None` when it can't be opened.
    fn open(&mut self, pid: u32) -> Option<Self::Process>;
    /// Every TCP listener now; `None` when the table can't be read.
    fn listeners(&mut self) -> Option<Vec<BoundListener>>;
    /// When `process` was created (`FILETIME` ticks).
    fn created(&mut self, process: &Self::Process) -> Option<u64>;
    /// Whether `process` is in the launch's job.
    fn in_job(&mut self, process: &Self::Process) -> Option<bool>;
}

/// Whether the process listening on `port` as `pid` is in the job: a handle to `pid` is opened
/// first, then the listeners are read again, and the port must still be `pid`'s, its process
/// created no later than its socket was bound (when Windows gives that time), before the job is
/// asked about that handle. `None` (outside) when any step fails.
pub fn listener_in_job(probe: &mut impl JobProbe, port: u16, pid: u32) -> Option<bool> {
    let process = probe.open(pid)?;
    let created = probe.created(&process)?;
    let row = probe
        .listeners()?
        .into_iter()
        .find(|row| row.port == port && row.pid == pid)?;
    if row.bound_at != 0 && created > row.bound_at {
        return None;
    }
    probe.in_job(&process)
}

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
/// a process `in_job` (port, process id) says is in this launch's job (`None`: it couldn't be
/// asked, so outside), and that each port in `expected` is one of those ports with such a
/// listener on it.
pub fn in_job_gate(
    slot: Slot,
    rows: &[(u16, u32)],
    expected: &[u16],
    mut in_job: impl FnMut(u16, u32) -> Option<bool>,
) -> Result<(), BrowserOff> {
    let (pool, daemon) = (slot.pool(), slot.daemon());
    if let Some(port) = expected
        .iter()
        .find(|port| !pool.contains(port) && !daemon.contains(port))
    {
        return Err(BrowserOff(format!(
            "port {port} isn't one of this matter's browser ports, so the agent can't reach it"
        )));
    }
    let mut ours = Vec::new();
    for (port, pid) in on_slot(slot, rows) {
        if in_job(*port, *pid) != Some(true) {
            return Err(BrowserOff(format!(
                "port {port}, one of this matter's browser ports, is held by a program outside \
                 this session (process {pid})"
            )));
        }
        ours.push(*port);
    }
    match expected.iter().find(|port| !ours.contains(port)) {
        None => Ok(()),
        Some(port) => Err(BrowserOff(format!(
            "nothing in this session listens on port {port}, one of this matter's browser \
             ports; the browser's helper may have moved to another port, which the agent can't \
             reach"
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
/// (which needs `JOB_OBJECT_QUERY`), with `expected` the ports the launch claimed for Chrome's
/// CDP, the stream server and the daemon. One read of the table, no wait. Call it once the
/// browser's listeners are up, and again when the stream server falls back to another port.
#[cfg(windows)]
pub fn check_browser_job(
    slot: Slot,
    job: std::os::windows::io::BorrowedHandle<'_>,
    expected: &[u16],
) -> Result<(), BrowserOff> {
    let mut probe = crate::windows::wfp::WindowsJobProbe::new(job);
    in_job_gate(slot, &listener_rows()?, expected, |port, pid| {
        listener_in_job(&mut probe, port, pid)
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
        let job = |_: u16, pid: u32| match pid {
            10 | 11 => Some(true),
            20 => Some(false),
            _ => None,
        };
        let ours = [(cdp, 10), (daemon, 11), (*s.host().start(), 99), (9000, 20)];
        assert_eq!(in_job_gate(s, &ours, &[cdp, daemon], job), Ok(()));
        let stale = [(cdp, 10), (daemon, 20)];
        let off = in_job_gate(s, &stale, &[cdp, daemon], job).unwrap_err();
        assert!(off.0.contains(&daemon.to_string()), "{off}");
        let unknown = [(cdp + 1, 30)];
        assert!(
            in_job_gate(s, &unknown, &[], job).is_err(),
            "a process it can't ask is outside"
        );
        // A stream server that fell back into the slot is checked again.
        let fallback = [(cdp, 10), (cdp + 2, 20)];
        assert!(in_job_gate(s, &fallback, &[cdp], job).is_err());
    }

    #[test]
    fn a_daemon_that_fell_back_to_an_ephemeral_port_turns_the_browser_off_plainly() {
        let s = slot(5);
        let (cdp, daemon) = (*s.pool().start(), *s.daemon().start());
        // Windows handed the daemon's preferred port out as a source port (no listener on it),
        // so agent-browser listens on an ephemeral port instead, from inside the job.
        let ephemeral = 51_234;
        let rows = [(cdp, 10), (ephemeral, 11)];
        let job = |_: u16, pid: u32| Some(pid == 10 || pid == 11);
        let expected = [cdp, daemon];
        let off = in_job_gate(s, &rows, &expected, job).unwrap_err();
        let text = off.to_string();
        assert!(
            text.starts_with("the browser is off for this run"),
            "{text}"
        );
        assert!(text.contains(&daemon.to_string()), "{text}");
        // The port it reports, given as expected, is refused as outside the slot.
        let moved = in_job_gate(s, &rows, &[cdp, ephemeral], job).unwrap_err();
        assert!(
            moved.0.contains("51234") && moved.0.contains("can't reach"),
            "{moved}"
        );
        // The preferred port held by a program outside the job: off too, with that program.
        let taken = [(cdp, 10), (daemon, 77), (ephemeral, 11)];
        let off = in_job_gate(s, &taken, &expected, job).unwrap_err();
        assert!(off.0.contains("77"), "{off}");
    }

    /// Processes by pid (creation time, in the job), and the listener rows each read returns.
    struct Fake {
        processes: Vec<(u32, u64, bool)>,
        reads: Vec<Vec<BoundListener>>,
        opened: Vec<u32>,
    }

    impl JobProbe for Fake {
        type Process = u32;
        fn open(&mut self, pid: u32) -> Option<u32> {
            self.opened.push(pid);
            self.processes.iter().any(|p| p.0 == pid).then_some(pid)
        }
        fn listeners(&mut self) -> Option<Vec<BoundListener>> {
            // The table is read only once a handle is held.
            assert_ne!(self.opened.len(), 0);
            (!self.reads.is_empty()).then(|| self.reads.remove(0))
        }
        fn created(&mut self, process: &u32) -> Option<u64> {
            self.processes.iter().find(|p| p.0 == *process).map(|p| p.1)
        }
        fn in_job(&mut self, process: &u32) -> Option<bool> {
            self.processes.iter().find(|p| p.0 == *process).map(|p| p.2)
        }
    }

    fn fake(processes: &[(u32, u64, bool)], rows: &[(u16, u32, u64)]) -> Fake {
        let rows = rows
            .iter()
            .map(|&(port, pid, bound_at)| BoundListener {
                port,
                pid,
                bound_at,
            })
            .collect();
        Fake {
            processes: processes.to_vec(),
            reads: vec![rows],
            opened: Vec::new(),
        }
    }

    #[test]
    fn the_job_is_asked_through_a_handle_held_while_the_row_is_read_again() {
        // The browser (pid 10, created at 100) bound port 46400 at 150: in the job.
        let mut ok = fake(&[(10, 100, true)], &[(46_400, 10, 150)]);
        assert_eq!(listener_in_job(&mut ok, 46_400, 10), Some(true));
        assert_eq!(ok.opened, [10]);
        // Windows doesn't give the bind time: the re-read pid still decides.
        let mut untimed = fake(&[(10, 100, true)], &[(46_400, 10, 0)]);
        assert_eq!(listener_in_job(&mut untimed, 46_400, 10), Some(true));
        // The process exited before the open: nothing to ask.
        let mut gone = fake(&[], &[(46_400, 10, 150)]);
        assert_eq!(listener_in_job(&mut gone, 46_400, 10), None);
        // The port changed hands between the first read and the open.
        let mut moved = fake(&[(10, 100, true)], &[(46_400, 11, 150)]);
        assert_eq!(listener_in_job(&mut moved, 46_400, 10), None);
        // The pid went to a process younger than the socket (a reused pid): outside.
        let mut reused = fake(&[(10, 200, true)], &[(46_400, 10, 150)]);
        assert_eq!(listener_in_job(&mut reused, 46_400, 10), None);
        // A table it can't read again: outside.
        let mut unread = fake(&[(10, 100, true)], &[]);
        unread.reads.clear();
        assert_eq!(listener_in_job(&mut unread, 46_400, 10), None);
        // Outside the job.
        let mut outside = fake(&[(10, 100, false)], &[(46_400, 10, 150)]);
        assert_eq!(listener_in_job(&mut outside, 46_400, 10), Some(false));
    }
}
