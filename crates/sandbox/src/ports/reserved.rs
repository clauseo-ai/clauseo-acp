//! The reserved-port check (`wfp::check_ports`, before every tier-B launch): the ports of the
//! launch's slot Windows refuses to bind on `127.0.0.1` or `::1` because an excluded port range
//! (Hyper-V, WSL and Docker reserve them) covers them.
//!
//! **Telling a reservation from a port in use.** A bind on an excluded port fails with
//! `WSAEACCES` (10013, `PermissionDenied`): that is what Docker, WSL and Hyper-V users report
//! for these ranges, and the VM row measures it on a set-up machine. A port another socket of
//! the same user holds fails with `WSAEADDRINUSE` (10048, `AddrInUse`), which never counts.
//! But `WSAEACCES` isn't only a reservation: Windows' socket security refuses a bind to a port
//! another user's socket holds, or one held with `SO_EXCLUSIVEADDRUSE`, with `WSAEACCES` too
//! (Microsoft's "Using `SO_REUSEADDR` and `SO_EXCLUSIVEADDRUSE`"). So the caller skips every
//! port in Windows' TCP table (any state) before the binds and drops any that shows up there
//! after them: a refused bind on a port no socket holds is read as a reservation.
//!
//! Windows publishes no call that lists the excluded ranges (`netsh int ipv4 show
//! excludedportrange` reads the TCP/IP stack's private store), and the documented reservation
//! calls, `CreatePersistentTcpPortReservation` and `LookupPersistentTcpPortReservation`, only
//! make or look up one exact range, so a bind is the test.
//!
//! The test socket binds and never listens ([`bind_only`]). An excluded range refuses the bind
//! itself, while set-up's filters act at `ALE_AUTH_LISTEN`: each slot's pool ports refuse a
//! listen by every token but the slot's user's, the lawyer's and SYSTEM's included, so a test
//! that listened would read every pool port as reserved and refuse every launch and set-up.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};

/// Binds a TCP socket to `ip:port` and closes it, without listening.
pub(crate) fn bind_only(ip: IpAddr, port: u16) -> io::Result<()> {
    let domain = if ip.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.bind(&SocketAddr::new(ip, port).into())
}

/// The ports of `ports`, `skip` aside, that `bind` refuses with `PermissionDenied`
/// (`WSAEACCES`) on `127.0.0.1` or `::1`. Any other failure (`AddrInUse`, `WSAEADDRINUSE`: a
/// socket holds the port) isn't a reservation.
pub(crate) fn refused_by(
    ports: impl Iterator<Item = u16>,
    skip: impl Fn(u16) -> bool,
    bind: impl Fn(IpAddr, u16) -> io::Result<()>,
) -> Vec<u16> {
    let refused = |port: u16| {
        [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ]
        .into_iter()
        .any(|ip| {
            matches!(bind(ip, port),
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
        })
    };
    ports
        .filter(|port| !skip(*port))
        .filter(|port| refused(*port))
        .collect()
}

/// The reserved ports of `ports`, by [`bind_only`]. Ports in `in_use`, which a socket holds,
/// and ports this process holds are skipped.
#[cfg(windows)]
pub(crate) fn refused_ports(
    ports: impl Iterator<Item = u16>,
    in_use: &std::collections::BTreeSet<u16>,
) -> Vec<u16> {
    let held = super::Held::lock();
    refused_by(
        ports,
        |port| in_use.contains(&port) || held.contains(port),
        bind_only,
    )
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn only_a_refused_bind_is_a_reservation_and_a_refused_listen_is_never_asked() {
        let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
        // Port 10 refuses ::1's bind (reserved); 11 is busy; 12 is skipped though reserved.
        // A filter that refuses only a listen leaves every bind fine, as for port 13.
        let bind = |ip: IpAddr, port: u16| match (port, ip.is_ipv6()) {
            (10, true) | (12, _) => Err(denied()),
            (11, _) => Err(io::Error::from(io::ErrorKind::AddrInUse)),
            _ => Ok(()),
        };
        assert_eq!(refused_by(10..=13, |port| port == 12, bind), [10]);
    }

    #[test]
    fn the_test_bind_never_listens_and_leaves_the_port_free() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let busy = bind_only(IpAddr::V4(Ipv4Addr::LOCALHOST), port).unwrap_err();
        assert_ne!(busy.kind(), io::ErrorKind::PermissionDenied);
        drop(listener);
        bind_only(IpAddr::V4(Ipv4Addr::LOCALHOST), port).unwrap();
        // Nothing listens there after it: a connect is refused.
        assert!(std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    }
}
