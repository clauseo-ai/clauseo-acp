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
/// session's Chrome (`browser`, none when it claimed no browser ports) are gone, for at least
/// `cap`. Past it, says which was still there.
///
/// The cap is counted in polls of real time, not on the host's clock: the processes it waits
/// for run in real time whatever that clock says, and tests move theirs by hand. Each poll
/// parks this thread, which nothing else unparks, for [`POLL`].
pub(super) fn wait_gone(
    tree: Option<&TreeWatch>,
    browser: Option<&BrowserSession>,
    cap: Duration,
) -> Result<(), Straggler> {
    let polls = cap.as_millis().div_ceil(POLL.as_millis());
    let mut polled = 0;
    loop {
        let Some(straggler) = still_there(tree, browser) else {
            return Ok(());
        };
        if polled >= polls {
            return Err(straggler);
        }
        polled += 1;
        std::thread::park_timeout(POLL);
    }
}

/// What is still there now, (a) first.
fn still_there(tree: Option<&TreeWatch>, browser: Option<&BrowserSession>) -> Option<Straggler> {
    if tree.is_some_and(|tree| !tree.gone()) {
        return Some(Straggler::AgentTree);
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
fn chrome_alive(session: &BrowserSession) -> Option<Straggler> {
    let cdp = SocketAddr::from((Ipv4Addr::LOCALHOST, session.ports.cdp.port()));
    match TcpStream::connect_timeout(&cdp, CONNECT_WAIT) {
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => None,
        Ok(_) | Err(_) => Some(Straggler::Chrome),
    }
}

/// The runs whose removal reached its cap: each keeps its proxy, relay and browser claims
/// (they go when the run drops) until the host stops ([`HeldRuns::release_all`]). Their folders
/// stay for the next start's sweep.
#[derive(Debug, Clone, Default)]
pub(crate) struct HeldRuns(Arc<Mutex<Vec<RunFolders>>>);

impl HeldRuns {
    /// Holds `run` until the host stops.
    pub(super) fn hold(&self, run: RunFolders) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(run);
    }

    /// Drops every held run, outside the lock: their proxies close and their relays stop.
    /// Answers how many there were.
    pub(crate) fn release_all(&self) -> usize {
        let held = std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner));
        let count = held.len();
        drop(held);
        count
    }
}

#[cfg(test)]
mod tests;
