//! Loopback listeners for one launch (the proxy's two, and stage E's connector relay), and
//! loopback ports claimed for a session that something else binds later (agent-browser's
//! daemon and Chrome's debugging port).
//!
//! macOS and Linux bind `127.0.0.1` on a port the OS picks. Windows binds a free port in
//! [`WINDOWS_PORTS`], the range that set-up's filters leave open and that each launch's dynamic
//! permit narrows to that launch's own ports (S15), starting at a random offset, with
//! `SO_EXCLUSIVEADDRUSE` set before each bind so no other process can bind the same port.
//!
//! A [`PortClaim`] holds a port in a process-wide registry until it drops. Nothing is bound
//! while it is held, so the registry keeps this process's own listeners and claims apart:
//! [`bind_loopback`], [`claim_port`] and [`claim_port_at`] never hand out a claimed port.
//!
//! On macOS and Linux a dropped claim retires its port instead of freeing it. Retired ports
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

/// The loopback ports a Windows listener may take.
pub const WINDOWS_PORTS: std::ops::RangeInclusive<u16> = 61080..=61179;

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
/// A claim holds only inside this process. Another local process can still bind the port
/// before Chrome or agent-browser does; the launch then fails, with no fallback to another
/// port. On macOS and Linux [`bind_loopback`] and [`claim_port`] bind port 0, so the kernel
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

/// Binds a listener on `127.0.0.1` for one launch, never on a claimed or retired port. On
/// Windows, when no port in [`WINDOWS_PORTS`] binds, the error names the range and says what
/// may hold it.
pub fn bind_loopback() -> io::Result<TcpListener> {
    imp::bind(&Held::lock())
}

/// Claims a free loopback port for the session. macOS and Linux: the port the OS gives a bind
/// of `127.0.0.1:0`, asked again when it is already claimed or retired. Windows: a free port in
/// [`WINDOWS_PORTS`] that is not claimed, tested with the same `SO_EXCLUSIVEADDRUSE` bind as
/// [`bind_loopback`]. The test listener is closed before this returns.
pub fn claim_port() -> io::Result<PortClaim> {
    let mut held = Held::lock();
    let port = imp::bind(&held)?.local_addr()?.port();
    held.claimed.insert(port);
    Ok(PortClaim { port })
}

/// Claims `port`, for a port whose number someone else chose (agent-browser's Windows daemon
/// port, a hash of its session name). [`io::ErrorKind::InvalidInput`] for port 0, and on
/// Windows for a port outside [`WINDOWS_PORTS`]; [`io::ErrorKind::AddrInUse`] when it is
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
    if !WINDOWS_PORTS.contains(&port) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "port {port} is outside {}..={}",
                WINDOWS_PORTS.start(),
                WINDOWS_PORTS.end()
            ),
        ));
    }
    claim_at_with(port, imp::bind_port)
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
    let mut last = io::Error::new(io::ErrorKind::AddrInUse, "no free port in 61080 to 61179");
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

/// The error when every port in [`WINDOWS_PORTS`] failed, keeping the last failure's kind.
#[cfg(any(windows, test))]
fn range_exhausted(last: &io::Error) -> io::Error {
    io::Error::new(
        last.kind(),
        format!(
            "no loopback port in {}..={} could be bound (last error: {last}); another program, \
             or a Windows port reservation (Hyper-V, WSL or Docker; see `netsh int ipv4 show \
             excludedportrange protocol=tcp`), may hold the range",
            WINDOWS_PORTS.start(),
            WINDOWS_PORTS.end()
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

    use super::{Held, WINDOWS_PORTS, first_free};

    /// The backlog of each listener.
    const BACKLOG: i32 = 128;

    /// A listener on a free port of [`WINDOWS_PORTS`] that `held` does not hold.
    pub(super) fn bind(held: &Held) -> io::Result<TcpListener> {
        let count = WINDOWS_PORTS.len();
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let offset = usize::from(u16::from_le_bytes(random)) % count;
        let ports = WINDOWS_PORTS.skip(offset).chain(WINDOWS_PORTS.take(offset));
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
mod tests {
    use super::*;

    #[test]
    fn binds_loopback() {
        let listener = bind_loopback().unwrap();
        let address = listener.local_addr().unwrap();
        assert!(address.ip().is_loopback(), "{address}");
        assert_ne!(address.port(), 0);
        #[cfg(windows)]
        assert!(WINDOWS_PORTS.contains(&address.port()), "{address}");
    }

    #[test]
    fn a_full_range_names_itself_and_the_windows_reservations() {
        let last = io::Error::from(io::ErrorKind::PermissionDenied);
        let error = range_exhausted(&last);
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        let text = error.to_string();
        assert!(text.contains("61080..=61179"), "{text}");
        assert!(text.contains("another program"), "{text}");
        assert!(
            text.contains("netsh int ipv4 show excludedportrange protocol=tcp"),
            "{text}"
        );
    }

    #[test]
    fn two_claims_differ_and_each_is_loopback_bindable() {
        let (a, b) = (claim_port().unwrap(), claim_port().unwrap());
        assert_ne!(a.port(), b.port());
        assert_ne!(a.port(), 0);
        #[cfg(windows)]
        assert!(WINDOWS_PORTS.contains(&a.port()), "{a:?}");
        assert!(format!("{a:?}").contains(&a.port().to_string()), "{a:?}");
    }

    #[test]
    fn a_claimed_port_is_never_handed_out_again() {
        let claim = claim_port().unwrap();
        for _ in 0..32 {
            let listener = bind_loopback().unwrap();
            assert_ne!(listener.local_addr().unwrap().port(), claim.port());
            assert_ne!(claim_port().unwrap().port(), claim.port());
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn a_bind_that_lands_on_a_claimed_port_is_asked_again() {
        let claim = claim_port().unwrap();
        // The claim binds nothing, so the test can put a listener on its port.
        let mut listeners = vec![imp::bind_port(claim.port()).unwrap()].into_iter();
        let got = imp::first_unheld(&Held::lock(), || {
            listeners.next().map_or_else(|| imp::bind_port(0), Ok)
        })
        .unwrap();
        assert_ne!(got.local_addr().unwrap().port(), claim.port());
        let all_claimed = imp::first_unheld(&Held::lock(), || imp::bind_port(claim.port()));
        assert!(all_claimed.is_err(), "the port was bound twice");
    }

    #[cfg(not(windows))]
    #[test]
    fn a_released_claim_is_not_handed_out_again() {
        let claim = claim_port().unwrap();
        let port = claim.port();
        drop(claim);
        let error = claim_port_at(port).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
        for _ in 0..200 {
            let next = claim_port().unwrap();
            assert_ne!(next.port(), port);
        }
        // The OS may hand the port back to a bind of port 0; the retired port is asked again.
        for _ in 0..200 {
            let listener = bind_loopback().unwrap();
            assert_ne!(listener.local_addr().unwrap().port(), port);
        }
    }

    #[test]
    fn the_candidate_loop_never_binds_a_claimed_port() {
        let claim = claim_port().unwrap();
        // The lock is held across the loop: the hook binds a socket and never touches the
        // registry.
        let held = Held::lock();
        // A free port other than the claim's, so the loop's only skip is the claimed one.
        let other = loop {
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            if port != claim.port() && !held.contains(port) {
                break port;
            }
        };
        let mut tried = Vec::new();
        let got = first_free([claim.port(), other].into_iter(), &held, |port| {
            tried.push(port);
            TcpListener::bind("127.0.0.1:0")
        })
        .unwrap();
        assert_eq!(tried, vec![other]);
        drop(got);
        let mut tried = Vec::new();
        let none = first_free([claim.port()].into_iter(), &held, |port| {
            tried.push(port);
            TcpListener::bind("127.0.0.1:0")
        });
        assert!(none.is_err());
        assert_eq!(tried, Vec::<u16>::new());
    }

    #[test]
    fn claim_at_checks_the_registry_before_its_test_bind() {
        let claim = claim_port().unwrap();
        let mut binds = 0;
        let error = claim_at_with(claim.port(), |_| {
            binds += 1;
            TcpListener::bind("127.0.0.1:0")
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
        assert_eq!(binds, 0);
    }

    #[test]
    fn claim_at_refuses_a_claimed_port_and_a_listening_one() {
        let claim = claim_port().unwrap();
        let error = claim_port_at(claim.port()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
        let listener = bind_loopback().unwrap();
        let port = listener.local_addr().unwrap().port();
        let error = claim_port_at(port).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}");
        let error = claim_port_at(0).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn dropping_a_claim_frees_its_port() {
        let claim = claim_port().unwrap();
        let port = claim.port();
        assert!(claim_port_at(port).is_err());
        drop(claim);
        let again = claim_port_at(port).unwrap();
        assert_eq!(again.port(), port);
    }

    #[cfg(windows)]
    #[test]
    fn claim_at_refuses_a_port_outside_the_windows_range() {
        for port in [61_079, 61_180, 80] {
            let error = claim_port_at(port).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{port}: {error}");
        }
        let start = claim_port_at(*WINDOWS_PORTS.start());
        let end = claim_port_at(*WINDOWS_PORTS.end());
        for claim in [start, end] {
            match claim {
                Ok(claim) => assert!(WINDOWS_PORTS.contains(&claim.port())),
                Err(error) => assert_eq!(error.kind(), io::ErrorKind::AddrInUse, "{error}"),
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_second_bind_with_reuseaddr_fails() {
        use socket2::{Domain, Protocol, Socket, Type};
        let listener = bind_loopback().unwrap();
        let address = listener.local_addr().unwrap();
        let other = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
        other.set_reuse_address(true).unwrap();
        assert!(
            other.bind(&address.into()).is_err(),
            "{address} bound twice"
        );
    }
}
