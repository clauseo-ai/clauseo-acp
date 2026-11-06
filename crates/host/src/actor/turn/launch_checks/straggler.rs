//! The wait before a session's run folder removal frees what the session holds: its egress
//! proxy, its case-law relay (R24), its browser claims and the folder itself. Off the actor, on
//! the removal's thread, it waits until both hold:
//!
//! - (a) the agent's tree is gone ([`TreeWatch::gone`]): on Unix the agent reaped and its
//!   process group empty (`killpg(group, 0)` answers `ESRCH`), on Windows its Job Object holding
//!   no running process. The tree's kill (`Handle::exited`, the shutdown ladder) is only sent,
//!   and after `StopStep::Unresponsive` the agent may still run.
//! - (b) when the session claimed browser ports, Chrome is gone: its debugging port refuses a
//!   connect ([`chrome_alive`]). After #534 Chrome leads a process group of its own, so the
//!   agent's tree kill can miss it, and it would go on using the proxy's port.
//!
//! It polls every [`POLL`] up to the cap (`HostConfig::with_tree_wait`, 20 s by default). Past
//! the cap nothing is freed: the run goes to [`HeldRuns`], which keeps its proxy, relay and
//! claims until the host stops, and the folder stays for the next start's sweep.
//!
//! Residuals: a descendant that calls `setsid` or `setpgid` leaves the agent's group, and (a)
//! does not see it (Unix). Chrome is covered by (b); anything else is the jail's (R17 PR-D/E).
//! Once the agent is reaped its group id can be reused by an unrelated group; then (a) never
//! holds, and the wait runs into the cap and fails closed.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_process::TreeWatch;

use super::RunFolders;
use crate::browser::session::BrowserSession;

/// How often the wait looks again.
const POLL: Duration = Duration::from_millis(50);

/// How long one connect to Chrome's debugging port may take. Loopback answers at once; a
/// connect that neither succeeds nor is refused counts as Chrome still there.
const CONNECT_WAIT: Duration = Duration::from_millis(200);

/// What was still there when the wait reached its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Straggler {
    /// (a): a process of the agent's tree.
    AgentTree,
    /// (b): something answers on the session's Chrome debugging port.
    Chrome,
}

impl Straggler {
    /// The log's word for it.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::AgentTree => "agent_tree",
            Self::Chrome => "chrome",
        }
    }
}

/// Waits, blocking, until the agent's tree (`tree`, none when the agent never started) and the
/// session's Chrome (`browser`, none when it claimed no browser ports) are gone, for about
/// `cap`. Past it, says which was still there. Once `stopping` answers true (the host is
/// stopping, [`HeldRuns::begin_stop`]) the wait gets at most [`STOP_GRACE`] more: at stop the
/// host frees every held run anyway, so waiting longer would only stall the quit. The grace
/// lets a group killed a moment ago finish dying, so a plain stop still removes its folder.
///
/// The cap is counted in polls of real time, not on the host's clock: the processes it waits
/// for run in real time whatever that clock says, and tests move theirs by hand. Each poll
/// parks this thread, which nothing else unparks, for [`POLL`], and a connect that timed out
/// is charged for its [`CONNECT_WAIT`] too ([`Budget`]), so the wait stays within about
/// `cap` plus one poll, inside its job's wait.
pub(super) fn wait_gone(
    tree: Option<&TreeWatch>,
    browser: Option<&BrowserSession>,
    cap: Duration,
    stopping: impl Fn() -> bool,
) -> Result<(), Straggler> {
    let mut budget = Budget::new(cap);
    loop {
        let Some(check) = still_there(tree, browser) else {
            return Ok(());
        };
        if check.slow {
            budget.charge(Budget::CONNECT_POLLS);
        }
        if stopping() {
            budget.end_within(STOP_GRACE);
        }
        if budget.spent() {
            return Err(check.straggler);
        }
        budget.charge(1);
        std::thread::park_timeout(POLL);
    }
}

/// How long a wait goes on once the host is stopping.
const STOP_GRACE: Duration = Duration::from_millis(250);

/// A wait's budget, in polls of [`POLL`]: each park costs one, and a look whose connect timed
/// out costs [`Budget::CONNECT_POLLS`] more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Budget {
    /// Polls left.
    left: u128,
}

impl Budget {
    /// The polls a timed-out connect's [`CONNECT_WAIT`] takes.
    const CONNECT_POLLS: u128 = CONNECT_WAIT.as_millis().div_ceil(POLL.as_millis());

    /// A budget of `cap`.
    fn new(cap: Duration) -> Self {
        Self {
            left: cap.as_millis().div_ceil(POLL.as_millis()),
        }
    }

    /// Takes `polls` from what is left.
    fn charge(&mut self, polls: u128) {
        self.left = self.left.saturating_sub(polls);
    }

    /// Leaves at most `grace`.
    fn end_within(&mut self, grace: Duration) {
        self.left = self.left.min(grace.as_millis().div_ceil(POLL.as_millis()));
    }

    /// Whether the budget is spent.
    fn spent(self) -> bool {
        self.left == 0
    }
}

/// One look: what is still there, and whether the look itself took a connect's whole wait.
#[derive(Debug, Clone, Copy)]
struct Check {
    straggler: Straggler,
    slow: bool,
}

/// What is still there now, (a) first.
fn still_there(tree: Option<&TreeWatch>, browser: Option<&BrowserSession>) -> Option<Check> {
    if tree.is_some_and(|tree| !tree.gone()) {
        return Some(Check {
            straggler: Straggler::AgentTree,
            slow: false,
        });
    }
    browser.and_then(chrome_alive)
}

/// (b): whether the session's Chrome may still run. Chrome's browser process is the only
/// listener on the debugging port the host claimed for it, so a refused connect means it is
/// gone. A second check, Chrome's process group gone, goes here once the guard exposes it.
///
/// Residual: until the guard exposes Chrome's process group (stage E follow-up), (b) can read
/// "gone" while Chrome is still starting: a session that ends before Chrome has bound its
/// debugging port finds the port refusing while Chrome lives.
fn chrome_alive(session: &BrowserSession) -> Option<Check> {
    let cdp = SocketAddr::from((Ipv4Addr::LOCALHOST, session.ports.cdp.port()));
    match TcpStream::connect_timeout(&cdp, CONNECT_WAIT) {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => None,
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Some(Check {
            straggler: Straggler::Chrome,
            slow: true,
        }),
        Ok(_) | Err(_) => Some(Check {
            straggler: Straggler::Chrome,
            slow: false,
        }),
    }
}

/// The runs whose removal reached its cap: each keeps its proxy, relay and browser claims
/// (they go when the run drops) until the host stops ([`HeldRuns::release_all`]). Their folders
/// stay for the next start's sweep. It also carries the host's stop flag to the removals'
/// threads ([`HeldRuns::begin_stop`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct HeldRuns {
    held: Arc<Mutex<Held>>,
    stopping: Arc<AtomicBool>,
}

/// The held runs, and whether the host has released them for good.
#[derive(Debug, Default)]
struct Held {
    runs: Vec<RunFolders>,
    closed: bool,
}

impl HeldRuns {
    /// The host is stopping (`begin_shutdown`): every removal's wait ends within [`STOP_GRACE`].
    pub(crate) fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    /// Whether the host is stopping.
    pub(super) fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// Holds `run` until the host stops, and answers true. After [`Self::release_all`] there is
    /// nobody left to free it, so it drops at once, outside the lock, and this answers false.
    pub(super) fn hold(&self, run: RunFolders) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        if held.closed {
            drop(held);
            drop(run);
            return false;
        }
        held.runs.push(run);
        true
    }

    /// Drops every held run, outside the lock: their proxies close and their relays stop. A
    /// later [`Self::hold`] drops its run at once. Answers how many there were.
    pub(crate) fn release_all(&self) -> usize {
        let runs = {
            let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
            held.closed = true;
            std::mem::take(&mut held.runs)
        };
        let count = runs.len();
        drop(runs);
        count
    }
}

#[cfg(test)]
mod tests;
