//! Loopback listeners for one launch: the proxy's two, and stage E's connector relay.
//!
//! macOS and Linux bind `127.0.0.1` on a port the OS picks. Windows binds a free port in
//! [`WINDOWS_PORTS`], the only loopback range S15's filters leave the sandbox user, starting at
//! a random offset, with `SO_EXCLUSIVEADDRUSE` set before each bind so no other process can
//! bind the same port.

use std::io;
use std::net::TcpListener;
#[cfg(not(windows))]
use std::net::{Ipv4Addr, SocketAddr};

/// The loopback ports a Windows listener may take.
pub const WINDOWS_PORTS: std::ops::RangeInclusive<u16> = 61080..=61179;

/// Binds a listener on `127.0.0.1` for one launch. On Windows, when no port in
/// [`WINDOWS_PORTS`] binds, the error names the range and says what may hold it.
pub fn bind_loopback() -> io::Result<TcpListener> {
    imp::bind()
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
    use super::{Ipv4Addr, SocketAddr, TcpListener, io};

    pub(super) fn bind() -> io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
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

    use super::{WINDOWS_PORTS, range_exhausted};

    /// The backlog of each listener.
    const BACKLOG: i32 = 128;

    pub(super) fn bind() -> io::Result<TcpListener> {
        let count = WINDOWS_PORTS.len();
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let offset = usize::from(u16::from_le_bytes(random)) % count;
        let ports = WINDOWS_PORTS.skip(offset).chain(WINDOWS_PORTS.take(offset));
        let mut last = io::Error::new(io::ErrorKind::AddrInUse, "no free port in 61080 to 61179");
        for port in ports {
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

    fn bind_port(port: u16) -> io::Result<TcpListener> {
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
