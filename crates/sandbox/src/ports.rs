//! Loopback listeners for one launch (the proxy's two, and stage E's connector relay), and
//! loopback ports claimed for a session that something else binds later (agent-browser's
//! daemon and Chrome's debugging port).
//!
//! macOS and Linux bind `127.0.0.1` on a port the OS picks. Windows binds a free port in
//! [`TIER_B_PORTS`], the range set-up's static permits open to every pool user (S15 PR7),
//! starting at a random offset, with `SO_EXCLUSIVEADDRUSE` set before each bind so no other
//! process can bind the same port. Every listener binds `127.0.0.1` only.
//!
//! The range is for tier B's listeners alone, since every pool user's agent reaches it: the
//! proxy's, R24's relay, and what a tier-B launch binds later ([`claim_port`],
//! [`claim_port_at`]). The browser's ports, and anything else that isn't tier B, are claimed
//! outside it ([`claim_browser_port`], [`claim_browser_port_at`]).
//!
//! A [`PortClaim`] holds a port in a process-wide registry until it drops. Nothing is bound
//! while it is held, so the registry keeps this process's own listeners and claims apart:
//! [`bind_loopback`] and the four claims never hand out a claimed port.
//!
//! On macOS and Linux a dropped claim retires its port instead of freeing it, and so does every
//! port [`bind_loopback`] hands out, as it binds. Retired ports
//! join a second registry that is never cleared, and those three functions never hand them out
//! again for the life of the process. The host's tree kill is `killpg`, which misses a jailed
//! child that called `setsid`; that child keeps its Seatbelt rules, and they still allow the
//! port. A port handed to the next session would then reach that child. Windows frees the port
//! on drop: its range is 100 ports, and its tree kill is a Job Object, so nothing survives it.

use std::collections::BTreeSet;
use std::io;
use std::net::TcpListener;
#[cfg(not(windows))]
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The loopback ports tier B's listeners take on Windows, and the only ones set-up's static
/// permits open to a pool user (S15 PR7). Below 49152, where Windows' dynamic range starts by
/// default, so the OS never hands them to another program's port-0 bind; clear of the
/// listeners known to sit below it (`WinRM` 47001, the `ASP.NET` state service 42424,
/// `EtherNet/IP` 44818, `BACnet` 47808, SQL Server's browser 1434) and of any port IANA
/// registers. Set-up and
/// every tier-B launch refuse a machine whose dynamic range, or whose excluded port ranges
/// (Hyper-V, WSL, Docker), overlap it (`wfp::check_ports`).
pub const TIER_B_PORTS: std::ops::RangeInclusive<u16> = 46100..=46199;

/// The ports claimed now, by live [`PortClaim`]s.
static CLAIMED: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// The ports of dropped claims on macOS and Linux, never handed out again for the life of the
/// process.
#[cfg(not(windows))]
static RETIRED: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());

/// The registry, locked. A panic while it was held leaves a set that is still whole.
fn claimed() -> MutexGuard<'static, BTreeSet<u16>> {
    CLAIMED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The retired registry, locked, with the same poison handling as [`claimed`].
#[cfg(not(windows))]
fn retired() -> MutexGuard<'static, BTreeSet<u16>> {
    RETIRED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The ports this process will not hand out, locked: the claimed ones and, on macOS and Linux,
/// the retired ones. Both are locked together, in this order, so a port moves from claimed to
/// retired with no gap in which a bind could take it.
struct Held {
    claimed: MutexGuard<'static, BTreeSet<u16>>,
    #[cfg(not(windows))]
    retired: MutexGuard<'static, BTreeSet<u16>>,
}

impl Held {
    fn lock() -> Self {
        Self {
            claimed: claimed(),
            #[cfg(not(windows))]
            retired: retired(),
        }
    }

    /// Whether `port` is claimed, or retired on macOS and Linux.
    fn contains(&self, port: u16) -> bool {
        #[cfg(not(windows))]
        if self.retired.contains(&port) {
            return true;
        }
        self.claimed.contains(&port)
    }
}

/// A free loopback port held for a session: no [`bind_loopback`], [`claim_port`] or
/// [`claim_port_at`] of this process hands it out while the claim lives. Nothing is bound;
/// whoever the port is for binds it.
///
/// A claim holds only inside this process, and nothing is bound while it is held: Chrome,
/// agent-browser's stream server and its Windows daemon bind at the browser's first tool call,
/// if ever. Another local process (another user's host) can bind the port first. The session's
/// own listener then fails or falls back to another port, which the browser guard's port files
/// catch, but the session's jail still allows the port, so its agent can reach whatever bound
/// it. A residual risk (stage C's review of #329): S15's listener rule closes it on Windows.
/// On macOS and Linux [`bind_loopback`] and [`claim_port`] bind port 0, so the kernel
/// picks the port and it is known only after the bind: if the kernel hands back a claimed or
/// retired port (it hands out only ports nobody holds), it is held for an instant, under the
/// registry lock, and the bind asked again. A launch binding that exact port in that instant
/// fails; it cannot take the port over. On Windows a claimed port is never bound.
///
/// On macOS and Linux a dropped claim retires its port for the life of the process; see the
/// module header. On Windows the port is freed when the claim drops.
#[derive(Debug)]
pub struct PortClaim {
    port: u16,
}

impl PortClaim {
    /// The claimed port.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for PortClaim {
    fn drop(&mut self) {
        // Windows frees the port: its tree kill is a Job Object, so no jailed child outlives it.
        #[cfg(windows)]
        claimed().remove(&self.port);
        #[cfg(not(windows))]
        {
            let mut held = Held::lock();
            held.claimed.remove(&self.port);
            held.retired.insert(self.port);
        }
    }
}

/// Binds a listener on `127.0.0.1` for one launch, never on a claimed or retired port. Its port
/// is retired as it binds: the listener holds it while it lives, and once it closes no later
/// launch of this process gets it. A jailed child of this launch that outlived it keeps the rule
/// for this port, and the proxy's browser listener takes no token.
#[cfg(not(windows))]
pub fn bind_loopback() -> io::Result<TcpListener> {
    let mut held = Held::lock();
    let listener = imp::bind(&held)?;
    held.retired.insert(listener.local_addr()?.port());
    Ok(listener)
}

/// Binds a listener on `127.0.0.1` for one tier-B launch, on a free port in [`TIER_B_PORTS`]
/// that is not claimed. When none binds, the error names the range and says what may hold it.
/// Windows retires nothing (see the module header).
#[cfg(windows)]
pub fn bind_loopback() -> io::Result<TcpListener> {
    imp::bind(&Held::lock())
}

/// Claims a free loopback port for a listener a tier-B launch binds later (tier B only: every
/// pool user reaches it on Windows; the browser claims with [`claim_browser_port`]). macOS and
/// Linux: the port the OS gives a bind of `127.0.0.1:0`, asked again when it is already claimed
/// or retired. Windows: a free port in [`TIER_B_PORTS`] that is not claimed, tested with the
/// same `SO_EXCLUSIVEADDRUSE` bind as [`bind_loopback`]. The test listener is closed before
/// this returns.
pub fn claim_port() -> io::Result<PortClaim> {
    let mut held = Held::lock();
    let port = imp::bind(&held)?.local_addr()?.port();
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// Claims `port`, whose number someone else chose, for tier B only (the browser's daemon port
/// goes through [`claim_browser_port_at`]). [`io::ErrorKind::InvalidInput`] for port 0, and on
/// Windows for a port outside [`TIER_B_PORTS`]; [`io::ErrorKind::AddrInUse`] when it is
/// claimed or retired, or a test bind of it fails. The caller picks another (a new session
/// name).
pub fn claim_port_at(port: u16) -> io::Result<PortClaim> {
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "port 0 cannot be claimed",
        ));
    }
    #[cfg(windows)]
    if !TIER_B_PORTS.contains(&port) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "port {port} is outside {}..={}",
                TIER_B_PORTS.start(),
                TIER_B_PORTS.end()
            ),
        ));
    }
    claim_at_with(port, imp::bind_port)
}

/// How many binds may land on a held or tier-B port before a browser claim gives up.
const BROWSER_TRIES: usize = 64;

/// Claims a free loopback port outside [`TIER_B_PORTS`], for a listener that isn't tier B's:
/// Chrome's debugging port, agent-browser's stream server. The port a bind of `127.0.0.1:0`
/// gets, asked again when it is claimed, retired, or in [`TIER_B_PORTS`] (where the OS hands
/// out ports only on a machine whose dynamic range covers them, which tier B refuses). The test
/// listener closes before this returns.
pub fn claim_browser_port() -> io::Result<PortClaim> {
    let mut held = Held::lock();
    let port = outside_tier_b(&held, || imp::bind_port(0))?
        .local_addr()?
        .port();
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// Claims `port`, whose number someone else chose (agent-browser's Windows daemon port, a hash
/// of its session name), for a listener that isn't tier B's. [`io::ErrorKind::InvalidInput`]
/// for port 0 or a port in [`TIER_B_PORTS`]; [`io::ErrorKind::AddrInUse`] when it is claimed
/// or retired, or a test bind of it fails. The caller picks another (a new session name).
pub fn claim_browser_port_at(port: u16) -> io::Result<PortClaim> {
    if port == 0 || TIER_B_PORTS.contains(&port) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "port {port} can't be claimed for the browser: 0 and {}..={} are refused",
                TIER_B_PORTS.start(),
                TIER_B_PORTS.end()
            ),
        ));
    }
    claim_at_with(port, imp::bind_port)
}

/// The first listener from `next` on a port `held` doesn't hold, outside [`TIER_B_PORTS`].
/// Listeners on other ports stay open until one is found, so the OS can't hand them back.
fn outside_tier_b(
    held: &Held,
    mut next: impl FnMut() -> io::Result<TcpListener>,
) -> io::Result<TcpListener> {
    let mut rejected = Vec::new();
    for _ in 0..BROWSER_TRIES {
        let listener = next()?;
        let port = listener.local_addr()?.port();
        if !held.contains(port) && !TIER_B_PORTS.contains(&port) {
            return Ok(listener);
        }
        rejected.push(listener);
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("{BROWSER_TRIES} loopback binds in a row landed on held or tier-B ports"),
    ))
}

/// [`claim_port_at`] after its range checks, with the test bind given as `bind_port`: the
/// registry is checked first, so a claimed port is never bound.
fn claim_at_with(
    port: u16,
    bind_port: impl FnOnce(u16) -> io::Result<TcpListener>,
) -> io::Result<PortClaim> {
    let mut held = Held::lock();
    if held.contains(port) {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("loopback port {port} is claimed or retired"),
        ));
    }
    drop(bind_port(port).map_err(|error| {
        io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("loopback port {port} cannot be bound: {error}"),
        )
    })?);
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// The first of `candidates` that `held` does not hold and `bind_port` binds. A claimed or
/// retired port is skipped before any bind; a port another socket holds (`AddrInUse`, or
/// `PermissionDenied` for a reserved one) is passed over; any other error ends the search.
#[cfg(any(windows, test))]
fn first_free(
    candidates: impl Iterator<Item = u16>,
    held: &Held,
    mut bind_port: impl FnMut(u16) -> io::Result<TcpListener>,
) -> io::Result<TcpListener> {
    let mut last = io::Error::new(io::ErrorKind::AddrInUse, "no free port in the tier-B range");
    for port in candidates.filter(|port| !held.contains(*port)) {
        match bind_port(port) {
            Ok(listener) => return Ok(listener),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                ) =>
            {
                last = error;
            }
            Err(error) => return Err(error),
        }
    }
    Err(range_exhausted(&last))
}

/// The error when every port in [`TIER_B_PORTS`] failed, keeping the last failure's kind.
#[cfg(any(windows, test))]
fn range_exhausted(last: &io::Error) -> io::Error {
    io::Error::new(
        last.kind(),
        format!(
            "no loopback port in {}..={} could be bound (last error: {last}); another program, \
             or a Windows port reservation (Hyper-V, WSL or Docker; see `netsh int ipv4 show \
             excludedportrange protocol=tcp`), may hold the range",
            TIER_B_PORTS.start(),
            TIER_B_PORTS.end()
        ),
    )
}

#[cfg(not(windows))]
mod imp {
    use super::{Held, Ipv4Addr, SocketAddr, TcpListener, io};

    /// How many binds may land on a claimed or retired port before [`bind`] gives up.
    const TRIES: usize = 64;

    /// A listener on a port the OS picks that `held` does not hold.
    pub(super) fn bind(held: &Held) -> io::Result<TcpListener> {
        first_unheld(held, || bind_port(0))
    }

    pub(super) fn bind_port(port: u16) -> io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
    }

    /// The first listener from `next` on a port `held` does not hold. Listeners on held ports
    /// stay open until one is found, so the OS cannot hand the same port back.
    pub(super) fn first_unheld(
        held: &Held,
        mut next: impl FnMut() -> io::Result<TcpListener>,
    ) -> io::Result<TcpListener> {
        let mut rejected = Vec::new();
        for _ in 0..TRIES {
            let listener = next()?;
            if !held.contains(listener.local_addr()?.port()) {
                return Ok(listener);
            }
            rejected.push(listener);
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("{TRIES} loopback binds in a row landed on claimed or retired ports"),
        ))
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::net::{Ipv4Addr, SocketAddr, TcpListener};
    use std::os::windows::io::AsRawSocket;

    use socket2::{Domain, Protocol, Socket, Type};
    use windows::Win32::Networking::WinSock::{
        SO_EXCLUSIVEADDRUSE, SOCKET, SOL_SOCKET, setsockopt,
    };

    use super::{Held, TIER_B_PORTS, first_free};

    /// The backlog of each listener.
    const BACKLOG: i32 = 128;

    /// A listener on a free port of [`TIER_B_PORTS`] that `held` does not hold.
    pub(super) fn bind(held: &Held) -> io::Result<TcpListener> {
        let count = TIER_B_PORTS.len();
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let offset = usize::from(u16::from_le_bytes(random)) % count;
        let ports = TIER_B_PORTS.skip(offset).chain(TIER_B_PORTS.take(offset));
        first_free(ports, held, bind_port)
    }

    pub(super) fn bind_port(port: u16) -> io::Result<TcpListener> {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        exclusive(&socket)?;
        socket.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)).into())?;
        socket.listen(BACKLOG)?;
        Ok(socket.into())
    }

    /// Sets `SO_EXCLUSIVEADDRUSE`, so no other socket can bind this address, `SO_REUSEADDR` or
    /// not.
    #[expect(
        unsafe_code,
        reason = "setsockopt has no safe wrapper in socket2 or windows"
    )]
    fn exclusive(socket: &Socket) -> io::Result<()> {
        let on: i32 = 1;
        let raw = SOCKET(usize::try_from(socket.as_raw_socket()).map_err(io::Error::other)?);
        // SAFETY: `raw` is the live socket `socket` owns for the whole call, and the option
        // value is a 4-byte BOOL that outlives the call, as setsockopt requires.
        let result = unsafe {
            setsockopt(
                raw,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                Some(&on.to_ne_bytes()),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(test)]
mod tests;
