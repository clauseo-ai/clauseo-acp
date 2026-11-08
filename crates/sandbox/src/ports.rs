//! Loopback listeners for one launch (the proxy's two, and stage E's connector relay), and
//! loopback ports claimed for a session that something else binds later (agent-browser's
//! daemon and Chrome's debugging port).
//!
//! macOS and Linux bind `127.0.0.1` on a port the OS picks. On Windows a tier-B launch's ports
//! are its pool user's slot's ([`Slot`], 0049 "Loopback ports per slot"), the only loopback
//! ports set-up's filters let that user reach:
//! - the host's listeners bind in the slot's 8 host ports ([`bind_loopback`] with
//!   [`ListenerFor::TierB`]: the proxy's agent and browser listeners, the R24 relay), starting
//!   at a random one, with `SO_EXCLUSIVEADDRUSE` set before each bind so no other process can
//!   bind the same port, and held for the run;
//! - Chrome's debugging port and agent-browser's stream server are claimed in its 8 pool ports
//!   ([`claim_pool_port`]), and agent-browser's daemon port in its 8 daemon ports
//!   ([`claim_daemon_port_at`], [`Slot::daemon`]). Only the slot's own user may listen on
//!   those, so the host never binds them, not even to test them: a claim reads Windows' TCP
//!   table instead. When all 8 are taken (an earlier run's connections in `TIME_WAIT`, or
//!   Windows lent a daemon port as another connection's source port), the claim fails with a
//!   plain reason and the browser is off for that run. [`check_browser_ports`] and
//!   [`check_browser_job`] gate the browser before and after it starts.
//!
//! A tier-C launch's listeners, and every browser claim of a launch that isn't tier B's, bind
//! or are claimed outside every slot's ports ([`ListenerFor::Other`], [`claim_browser_port`],
//! [`claim_browser_port_at`]). Every listener binds `127.0.0.1` only, so clients must name
//! the literal `127.0.0.1`: `localhost` may resolve to `::1`, which no filter opens.
//!
//! A [`PortClaim`] holds a port in a process-wide registry until it drops. Nothing is bound
//! while it is held, so the registry keeps this process's own listeners and claims apart:
//! [`bind_loopback`] and the claims never hand out a claimed port.
//!
//! On macOS and Linux a dropped claim retires its port instead of freeing it, and so does every
//! port [`bind_loopback`] hands out, as it binds. Retired ports
//! join a second registry that is never cleared, and those functions never hand them out
//! again for the life of the process. The host's tree kill is `killpg`, which misses a jailed
//! child that called `setsid`; that child keeps its Seatbelt rules, and they still allow the
//! port. A port handed to the next session would then reach that child. Windows frees the port
//! on drop: a slot has 8 ports per block, and its tree kill is a Job Object, so nothing survives
//! it.

use std::collections::BTreeSet;
use std::io;
use std::net::TcpListener;
#[cfg(not(windows))]
use std::net::{Ipv4Addr, SocketAddr};
use std::ops::RangeInclusive;
use std::sync::{Mutex, MutexGuard, PoisonError};

mod gate;
mod slot;

pub use gate::{BrowserOff, idle_at_launch, in_job_gate};
#[cfg(windows)]
pub use gate::{check_browser_job, check_browser_ports};
pub use slot::{DAEMON_BLOCK, HOST_BLOCK, POOL_BLOCK, SLOT_PORTS, Slot};

/// Blocks H and P together, the slots' ports below Windows' dynamic range (49152 by default),
/// so the OS never hands them to another program's port-0 bind; clear of the listeners known
/// to sit below it (`WinRM` 47001, the `ASP.NET` state service 42424, `EtherNet/IP` 44818,
/// `BACnet` 47808, SQL Server's browser 1434). IANA's registry for 46200-46611 is still to be
/// checked (0049). Set-up and every tier-B launch refuse a machine whose dynamic range, or
/// whose excluded port ranges (Hyper-V, WSL, Docker), overlap it, or where another program
/// listens in it (`wfp::check_ports`). Block D sits inside the dynamic range by design
/// ([`DAEMON_BLOCK`]).
pub const TIER_B_PORTS: RangeInclusive<u16> = 46_100..=46_611;

/// Whether `port` is one of any slot's: [`TIER_B_PORTS`], and block D on Windows. Nothing but
/// a tier-B launch takes them.
fn slots_hold(port: u16) -> bool {
    TIER_B_PORTS.contains(&port) || (cfg!(windows) && DAEMON_BLOCK.contains(&port))
}

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

/// A free loopback port held for a session: no [`bind_loopback`] or claim of this process hands
/// it out while the claim lives. Nothing is bound; whoever the port is for binds it.
///
/// A claim holds only inside this process, and nothing is bound while it is held: Chrome,
/// agent-browser's stream server and its Windows daemon bind at the browser's first tool call,
/// if ever. Another local process (another user's host) can bind the port first. The session's
/// own listener then fails or falls back to another port, which the browser guard's port files
/// catch, but the session's jail still allows the port, so its agent can reach whatever bound
/// it. A residual risk (stage C's review of #329): on Windows set-up's filters let only the
/// slot's user listen on its pool ports and no other pool user on its daemon ports, and
/// [`check_browser_job`] refuses a listener outside the launch's job.
/// On macOS and Linux [`bind_loopback`] and [`claim_pool_port`] bind port 0, so the kernel
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

/// Who a loopback listener is for, which decides its port on Windows (S15 PR7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerFor {
    /// A tier-B launch's agent, whose pool user has `slot`: on Windows a free port of the slot's
    /// host ports ([`Slot::host`]), which only that user's agent reaches. It still checks a
    /// secret where it has one (the proxy's agent listener, the relay).
    TierB {
        /// The launch's pool user's slot.
        slot: Slot,
    },
    /// Anything else: a tier-C launch's listeners. The port a bind of `127.0.0.1:0` gets,
    /// asked again when it lands on any slot's port or on a held port, so no pool user's
    /// filters reach it.
    Other,
}

/// Binds a listener on `127.0.0.1` for one launch, for `whom` ([`ListenerFor`]), never on a
/// claimed or retired port. Its port is retired as it binds: the listener holds it while it
/// lives, and once it closes no later launch of this process gets it. A jailed child of this
/// launch that outlived it keeps the rule for this port, and the proxy's browser listener takes
/// no token.
#[cfg(not(windows))]
pub fn bind_loopback(whom: ListenerFor) -> io::Result<TcpListener> {
    let mut held = Held::lock();
    let listener = match whom {
        ListenerFor::TierB { .. } => imp::bind(&held)?,
        ListenerFor::Other => outside_slots(&held, || imp::bind_port(0))?,
    };
    held.retired.insert(listener.local_addr()?.port());
    Ok(listener)
}

/// Binds a listener on `127.0.0.1` for one launch, for `whom` ([`ListenerFor`]): a tier-B
/// launch's on a free port of its slot's host ports that is not claimed (when none binds, the
/// launch refuses: the error names the 8 ports and says what may hold them), anything else's
/// outside every slot's. Windows retires nothing (see the module header).
#[cfg(windows)]
pub fn bind_loopback(whom: ListenerFor) -> io::Result<TcpListener> {
    let held = Held::lock();
    match whom {
        ListenerFor::TierB { slot } => imp::bind(&held, slot.host()),
        ListenerFor::Other => outside_slots(&held, || imp::bind_port(0)),
    }
}

/// Claims a free port of `slot`'s pool ports ([`Slot::pool`]) for a listener its tier-B
/// launch's pool user binds later: Chrome's debugging port, agent-browser's stream server.
/// macOS and Linux: the port the OS gives a bind of `127.0.0.1:0`, asked again when it is
/// already claimed or retired; the slot isn't used. Windows: the first of the 8, from a random
/// one, that this process doesn't hold and that Windows' TCP table shows no socket on. Nothing is
/// bound: only the slot's user may listen there. When none is free the error says so plainly,
/// and the browser is off for that run.
pub fn claim_pool_port(slot: Slot) -> io::Result<PortClaim> {
    let mut held = Held::lock();
    let port = imp::claim_pool(&held, slot)?;
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// Claims `port`, one of `slot`'s daemon ports ([`Slot::daemon`]), for agent-browser's Windows
/// daemon, whose port is a hash of its session name: the host draws names until one hashes into
/// [`Slot::daemon`] and this claims it. [`io::ErrorKind::InvalidInput`] for port 0, and on
/// Windows for a port outside the slot's daemon ports; [`io::ErrorKind::AddrInUse`] when it is
/// claimed or retired, or (Windows) Windows' TCP table shows a socket on it, as when Windows lent
/// it to another connection as its source port, or (macOS and Linux) a test bind of it fails.
/// The caller draws another name. Windows binds nothing here either.
pub fn claim_daemon_port_at(slot: Slot, port: u16) -> io::Result<PortClaim> {
    if port == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "port 0 cannot be claimed",
        ));
    }
    #[cfg(windows)]
    {
        let daemon = slot.daemon();
        if !daemon.contains(&port) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "port {port} isn't one of {slot}'s daemon ports, {}..={}",
                    daemon.start(),
                    daemon.end()
                ),
            ));
        }
        let mut held = Held::lock();
        let in_use = imp::in_use(&daemon)?;
        pick_free(std::iter::once(port), &held, &in_use)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("loopback port {port} is claimed or in use"),
                )
            })
            .map(|port| {
                held.claimed.insert(port);
                PortClaim { port }
            })
    }
    #[cfg(not(windows))]
    {
        let _ = slot;
        claim_at_with(port, imp::bind_port)
    }
}

/// The first of `candidates` that `held` doesn't hold and `in_use` doesn't list.
#[cfg(any(windows, test))]
fn pick_free(
    mut candidates: impl Iterator<Item = u16>,
    held: &Held,
    in_use: &BTreeSet<u16>,
) -> Option<u16> {
    candidates.find(|port| !held.contains(*port) && !in_use.contains(port))
}

/// `ports` from the one `offset` places in, round to the one before it.
#[cfg(any(windows, test))]
fn rotated(ports: &RangeInclusive<u16>, offset: usize) -> Vec<u16> {
    let count = ports.len().max(1);
    let offset = offset % count;
    ports
        .clone()
        .skip(offset)
        .chain(ports.clone().take(offset))
        .collect()
}

/// The plain reason when none of a slot's 8 ports in `ports` is free.
#[cfg(any(windows, test))]
fn slot_exhausted(ports: &RangeInclusive<u16>, what: &str, last: Option<&io::Error>) -> io::Error {
    let last = last
        .map(|error| format!(" (last error: {error})"))
        .unwrap_or_default();
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!(
            "all {SLOT_PORTS} of this matter's {what} ports, {}..={}, are in use{last}: \
             connections from an earlier run may still be closing (a few minutes), or another \
             program, or a Windows port reservation (Hyper-V, WSL or Docker; see `netsh int \
             ipv4 show excludedportrange protocol=tcp`), holds them",
            ports.start(),
            ports.end()
        ),
    )
}

/// The ports of [`TIER_B_PORTS`] Windows refuses to bind on `127.0.0.1` or `::1`
/// (`PermissionDenied`, `WSAEACCES`): an excluded port range (Hyper-V, WSL and Docker reserve
/// them). Ports in `listening`, which a listener holds, and ports this process holds are
/// skipped; each test bind closes at once. Windows publishes no call that lists the excluded
/// ranges (`netsh int ipv4 show excludedportrange` reads the TCP/IP stack's private store), and
/// the documented reservation calls, `CreatePersistentTcpPortReservation` and
/// `LookupPersistentTcpPortReservation`, only make or look up one exact range, so a bind is the
/// test.
#[cfg(windows)]
pub(crate) fn refused_ports(listening: &BTreeSet<u16>) -> Vec<u16> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    let held = Held::lock();
    let refused = |port: u16| {
        [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ]
        .into_iter()
        .any(|ip| {
            matches!(TcpListener::bind((ip, port)),
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
        })
    };
    TIER_B_PORTS
        .filter(|port| !listening.contains(port) && !held.contains(*port))
        .filter(|port| refused(*port))
        .collect()
}

/// How many binds may land on a held or slot port before a browser claim gives up.
const BROWSER_TRIES: usize = 64;

/// Claims a free loopback port outside every slot's, for a listener that isn't a tier-B
/// launch's: Chrome's debugging port, agent-browser's stream server. The port a bind of
/// `127.0.0.1:0` gets, asked again when it is claimed, retired, in [`TIER_B_PORTS`] (where the
/// OS hands out ports only on a machine whose dynamic range covers them, which tier B refuses),
/// or on Windows in [`DAEMON_BLOCK`]. The test listener closes before this returns.
pub fn claim_browser_port() -> io::Result<PortClaim> {
    let mut held = Held::lock();
    let port = outside_slots(&held, || imp::bind_port(0))?
        .local_addr()?
        .port();
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// Claims `port`, whose number someone else chose (agent-browser's Windows daemon port, a hash
/// of its session name), for a listener that isn't a tier-B launch's.
/// [`io::ErrorKind::InvalidInput`] for port 0 or a port in [`TIER_B_PORTS`];
/// [`io::ErrorKind::AddrInUse`] when it is claimed or retired, a test bind of it fails, or on
/// Windows it is in [`DAEMON_BLOCK`] (so a name search draws again). The caller picks another
/// (a new session name).
pub fn claim_browser_port_at(port: u16) -> io::Result<PortClaim> {
    #[cfg(windows)]
    if DAEMON_BLOCK.contains(&port) {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("port {port} is held for the sandbox's slots"),
        ));
    }
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

/// The first listener from `next` on a port `held` doesn't hold, outside every slot's
/// ([`slots_hold`]). Listeners on other ports stay open until one is found, so the OS can't
/// hand them back.
fn outside_slots(
    held: &Held,
    mut next: impl FnMut() -> io::Result<TcpListener>,
) -> io::Result<TcpListener> {
    let mut rejected = Vec::new();
    for _ in 0..BROWSER_TRIES {
        let listener = next()?;
        let port = listener.local_addr()?.port();
        if !held.contains(port) && !slots_hold(port) {
            return Ok(listener);
        }
        rejected.push(listener);
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("{BROWSER_TRIES} loopback binds in a row landed on held or slot ports"),
    ))
}

/// [`claim_browser_port_at`] after its range checks, with the test bind given as `bind_port`: the
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

/// The first of `candidates` (a slot's host ports, `ports`) that `held` does not hold and
/// `bind_port` binds. A claimed or retired port is skipped before any bind; a port another
/// socket holds (`AddrInUse`, or `PermissionDenied` for a reserved one) is passed over; any
/// other error ends the search. When none binds, [`slot_exhausted`] says why.
#[cfg(any(windows, test))]
fn first_free(
    candidates: impl Iterator<Item = u16>,
    ports: &RangeInclusive<u16>,
    held: &Held,
    mut bind_port: impl FnMut(u16) -> io::Result<TcpListener>,
) -> io::Result<TcpListener> {
    let mut last = None;
    for port in candidates.filter(|port| !held.contains(*port)) {
        match bind_port(port) {
            Ok(listener) => return Ok(listener),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                ) =>
            {
                last = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(slot_exhausted(ports, "host", last.as_ref()))
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

    /// A pool claim's port: one the OS picks that `held` does not hold.
    pub(super) fn claim_pool(held: &Held, _slot: super::Slot) -> io::Result<u16> {
        Ok(bind(held)?.local_addr()?.port())
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
    use std::collections::BTreeSet;
    use std::io;
    use std::net::{Ipv4Addr, SocketAddr, TcpListener};
    use std::ops::RangeInclusive;
    use std::os::windows::io::AsRawSocket;

    use socket2::{Domain, Protocol, Socket, Type};
    use windows::Win32::Networking::WinSock::{
        SO_EXCLUSIVEADDRUSE, SOCKET, SOL_SOCKET, setsockopt,
    };

    use super::{Held, Slot, first_free, pick_free, rotated, slot_exhausted};

    /// The backlog of each listener.
    const BACKLOG: i32 = 128;

    /// A random offset into a slot's 8 ports.
    fn offset() -> io::Result<usize> {
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(usize::from(u16::from_le_bytes(random)))
    }

    /// A listener on a free port of `ports` (a slot's host ports) that `held` does not hold,
    /// trying them from a random one.
    pub(super) fn bind(held: &Held, ports: RangeInclusive<u16>) -> io::Result<TcpListener> {
        first_free(
            rotated(&ports, offset()?).into_iter(),
            &ports,
            held,
            bind_port,
        )
    }

    /// The ports of `ports` Windows' TCP table shows a socket on, any state and address.
    pub(super) fn in_use(ports: &RangeInclusive<u16>) -> io::Result<BTreeSet<u16>> {
        let all =
            crate::windows::wfp::ports_in_use().map_err(|gap| io::Error::other(gap.to_string()))?;
        Ok(all
            .into_iter()
            .filter(|port| ports.contains(port))
            .collect())
    }

    /// A pool claim's port: the first of `slot`'s pool ports, from a random one, that `held`
    /// does not hold and no socket is on. Nothing is bound.
    pub(super) fn claim_pool(held: &Held, slot: Slot) -> io::Result<u16> {
        let ports = slot.pool();
        let in_use = in_use(&ports)?;
        pick_free(rotated(&ports, offset()?).into_iter(), held, &in_use)
            .ok_or_else(|| slot_exhausted(&ports, "browser", None))
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
