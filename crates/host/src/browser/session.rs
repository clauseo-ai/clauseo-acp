//! One browser session's loopback ports and socket folder (S27, decision record 0029), claimed on
//! the launch-prep thread and held until the agent's process tree was killed. Two processes can
//! outlive that kill and with it the claims: an agent that ignores it (the process crate's
//! `StopStep::Unresponsive`), and agent-browser's Unix daemon, which leaves the tree (`setsid`)
//! and lives until its idle timeout (G17 item 6). On Windows the guard's own Job Object ends
//! the daemon and Chrome when the guard exits. Claims hold within this host process only: two
//! hosts on one machine can be handed the same port (stage C's `ports`).
//!
//! - [`BrowserPorts`]: Chrome's debugging port (`--cdp-port`), the stream server's port
//!   (`AGENT_BROWSER_STREAM_PORT`) and, on Windows, agent-browser's daemon port
//!   (`--daemon-port`), each a [`PortClaim`] from stage C's `ports` (`claim_port`, or
//!   `claim_port_at` for the daemon's), so no other session of this host is handed them. The
//!   jail pins the session's loopback reach to them ([`BrowserPorts::loopback`]).
//! - macOS and Linux: one [`BrowserSocketDir`], `/tmp/clauseo-ab-<16 random>` (mode 0700, with
//!   its lock file), as `AGENT_BROWSER_SOCKET_DIR`. The guard gets [`BrowserSocketDir::path`],
//!   the `/tmp/...` form, never the canonical one, which is 8 bytes longer on macOS
//!   (`/private/tmp/...`) and counts against the 103-byte socket path.
//! - agent-browser's session name ([`BrowserSession::name`]): the session's id on macOS and
//!   Linux. On Windows its daemon listens on a port it derives from the name
//!   ([`daemon_port_of`]), so the host draws random names until the port is one in the jail's
//!   range that it can claim ([`search_name`]); agent-browser issue vercel-labs/agent-browser#2313
//!   would let the host pass the port instead.
//!
//! Dropping a [`BrowserSession`] releases the claims and removes the folder. The launch's
//! `RunFolders` holds it, and it drops off the actor when the run folder goes
//! (`launch_checks::discard`): after the agent's tree was killed, or when the agent never
//! started.

use std::io;
#[cfg(any(windows, test))]
use std::ops::RangeInclusive;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use clauseo_sandbox::BrowserSocketDir;
use clauseo_sandbox::ports::{self, PortClaim};

/// The loopback ports the host claimed for one browser session.
#[derive(Debug)]
pub(crate) struct BrowserPorts {
    /// Chrome's debugging port, the guard's `--cdp-port`.
    pub(crate) cdp: PortClaim,
    /// The stream server's port, `AGENT_BROWSER_STREAM_PORT`.
    pub(crate) stream: PortClaim,
    /// Windows only: agent-browser's daemon port, the guard's `--daemon-port`.
    pub(crate) daemon: Option<PortClaim>,
}

impl BrowserPorts {
    /// The ports the session's jail may reach on loopback, in order CDP, stream, then the
    /// daemon's on Windows: what stage C's R17 maps to `Loopback::Ports` where it builds the
    /// session's policy.
    pub(crate) fn loopback(&self) -> Vec<u16> {
        let mut ports = vec![self.cdp.port(), self.stream.port()];
        ports.extend(self.daemon.as_ref().map(PortClaim::port));
        ports
    }
}

/// One browser session's claims: its name, its ports and, on macOS and Linux, its socket
/// folder. Dropping it releases the ports and removes the folder; drop it only after the
/// agent's process tree was killed (see the module's documentation for what can outlive that).
#[derive(Debug)]
pub(crate) struct BrowserSession {
    /// agent-browser's session name, `{session_id}`.
    pub(crate) name: String,
    /// The claimed ports.
    pub(crate) ports: BrowserPorts,
    /// The socket folder, `{socket_dir}`; R17 hands it to `PolicyBuilder::browser_socket_dir`.
    #[cfg(unix)]
    pub(crate) socket_dir: BrowserSocketDir,
}

/// Why a browser session could not be claimed. The browser is left out
/// (`BrowserNotInstalled`) and the launch goes on.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ClaimError {
    /// A loopback port could not be claimed.
    #[error("no loopback port could be claimed for the browser: {0}")]
    Port(io::Error),
    /// The socket folder could not be made (Unix only: Windows uses `<cwd>\run`).
    #[cfg(unix)]
    #[error("the browser's socket folder could not be made: {0}")]
    SocketDir(io::Error),
    /// A random session name could not be drawn.
    #[cfg(any(windows, test))]
    #[error("no random session name could be drawn for the browser: {0}")]
    Name(io::Error),
    /// No name drawn hashed to a port the host could claim.
    #[cfg(any(windows, test))]
    #[error(
        "none of {draws} session names drawn gave agent-browser a daemon port the host could \
         claim in {first}..={last}"
    )]
    NameSearchExhausted {
        /// How many names were drawn.
        draws: usize,
        /// The range's first port.
        first: u16,
        /// The range's last port.
        last: u16,
    },
}

impl BrowserSession {
    /// Claims `session_id`'s browser session: on macOS and Linux its socket folder (in `/tmp`,
    /// or in `socket_root` for tests) and two ports, with the session id as the name; on
    /// Windows a drawn name with its daemon port, then two ports. Socket and file system calls:
    /// off the actor.
    pub(crate) fn claim(
        session_id: &str,
        #[cfg(unix)] socket_root: Option<&Path>,
    ) -> Result<Self, ClaimError> {
        #[cfg(unix)]
        let socket_dir = match socket_root {
            Some(root) => BrowserSocketDir::create_in(root),
            None => BrowserSocketDir::create(),
        }
        .map_err(ClaimError::SocketDir)?;
        #[cfg(unix)]
        let (name, daemon) = (session_id.to_owned(), None);
        #[cfg(windows)]
        let (name, daemon) = {
            // agent-browser's name is drawn, not the session's id.
            let (name, claim) = search_name(
                random_name,
                ports::claim_port_at,
                &ports::WINDOWS_PORTS,
                MAX_DRAWS,
            )?;
            tracing::debug!(%session_id, %name, port = claim.port(), "drew agent-browser's session name");
            (name, Some(claim))
        };
        let cdp = ports::claim_port().map_err(ClaimError::Port)?;
        let stream = ports::claim_port().map_err(ClaimError::Port)?;
        Ok(Self {
            name,
            ports: BrowserPorts {
                cdp,
                stream,
                daemon,
            },
            #[cfg(unix)]
            socket_dir,
        })
    }
}

/// macOS and Linux, at host start: removes the browser socket folders (and their lock files)
/// an earlier host left in `/tmp` (or `root`, for tests), through
/// [`BrowserSocketDir::sweep_stale`]. No launch of this host is live yet, so no id is kept; a
/// folder whose lock another live host holds is left. Runs once the store is open, after which
/// the host is the data folder's only one. A failure to read the folder is a warning.
#[cfg(unix)]
pub(crate) fn sweep_stale(root: Option<&Path>) {
    let swept = match root {
        Some(root) => BrowserSocketDir::sweep_stale_in(root, |_| false),
        None => BrowserSocketDir::sweep_stale(|_| false),
    };
    match swept {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "removed stale browser socket folders"),
        Err(error) => tracing::warn!(%error, "the stale browser socket folders were not swept"),
    }
}

/// The first port agent-browser's Windows daemon port can be (`connection.rs`).
#[cfg(any(windows, test))]
const DAEMON_PORT_BASE: u32 = 49_152;
/// How many ports above [`DAEMON_PORT_BASE`] the hash can land on.
#[cfg(any(windows, test))]
const DAEMON_PORT_SPAN: u32 = 16_383;
/// How many names [`search_name`] draws before it gives up. A name lands in the jail's 100
/// ports about once in 164 draws.
#[cfg(any(windows, test))]
pub(crate) const MAX_DRAWS: usize = 200_000;
/// The letters of a drawn name: the guard's session charset, `A-Z a-z 0-9 _ -`, 64 of them.
#[cfg(any(windows, test))]
const NAME_LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
/// A drawn name's length (the guard allows at most 64).
#[cfg(any(windows, test))]
const NAME_LEN: usize = 16;

/// agent-browser's Windows daemon port for the session `name` (v0.41.1 `connection.rs`
/// `get_port_for_session`, with no namespace): `h = 0`; for each character
/// `h = (h << 5) - h + c` in wrapping `i32`; the port is `49152 + |h| % 16383`.
#[cfg(any(windows, test))]
pub(crate) fn daemon_port_of(name: &str) -> u16 {
    let mut hash: i32 = 0;
    for c in name.chars() {
        // `c as i32`, as agent-browser has it: a char is at most 0x10FFFF, so this is exact.
        let code = i32::try_from(u32::from(c)).unwrap_or(i32::MAX);
        hash = (hash << 5).wrapping_sub(hash).wrapping_add(code);
    }
    let offset = hash.unsigned_abs() % DAEMON_PORT_SPAN;
    u16::try_from(DAEMON_PORT_BASE + offset).unwrap_or(u16::MAX)
}

/// Draws names with `draw` until one's [`daemon_port_of`] is in `range` and `claim` claims
/// it, at most `max` draws. A port `claim` finds taken (`AddrInUse`) means another draw; any
/// other error from `claim` or `draw` ends the search.
#[cfg(any(windows, test))]
pub(crate) fn search_name<T>(
    mut draw: impl FnMut() -> io::Result<String>,
    mut claim: impl FnMut(u16) -> io::Result<T>,
    range: &RangeInclusive<u16>,
    max: usize,
) -> Result<(String, T), ClaimError> {
    for _ in 0..max {
        let name = draw().map_err(ClaimError::Name)?;
        let port = daemon_port_of(&name);
        if !range.contains(&port) {
            continue;
        }
        match claim(port) {
            Ok(claimed) => return Ok((name, claimed)),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {}
            Err(error) => return Err(ClaimError::Port(error)),
        }
    }
    Err(ClaimError::NameSearchExhausted {
        draws: max,
        first: *range.start(),
        last: *range.end(),
    })
}

/// A random name of [`NAME_LEN`] letters from [`NAME_LETTERS`], from the OS's random source.
#[cfg(any(windows, test))]
pub(crate) fn random_name() -> io::Result<String> {
    let mut bytes = [0_u8; NAME_LEN];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes
        .iter()
        .filter_map(|byte| NAME_LETTERS.get(usize::from(byte & 63)).copied())
        .map(char::from)
        .collect())
}

#[cfg(test)]
mod tests;
