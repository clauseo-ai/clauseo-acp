//! [`bind_loopback_exclusive`]: the sign-in callback's listener, and R24's relay's.

use std::io;
use std::net::{Ipv4Addr, TcpListener};

#[cfg(windows)]
mod windows;

/// A listener on `127.0.0.1:port` (`0` picks a free port), never on another address. On Windows
/// the socket gets `SO_EXCLUSIVEADDRUSE` before it binds (RFC 8252 appendix B.3), so no other
/// process of the same user can bind the same port with `SO_REUSEADDR` and catch the code. On
/// macOS and Linux a bound port cannot be taken that way, so nothing else is set.
pub fn bind_loopback_exclusive(port: u16) -> io::Result<TcpListener> {
    #[cfg(windows)]
    {
        windows::bind_exclusive(port)
    }
    #[cfg(not(windows))]
    {
        TcpListener::bind((Ipv4Addr::LOCALHOST, port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_loopback_on_a_free_port() {
        let listener = bind_loopback_exclusive(0).unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(addr.port(), 0);
    }

    #[test]
    fn a_second_bind_of_the_same_port_fails() {
        let listener = bind_loopback_exclusive(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(bind_loopback_exclusive(port).is_err());
    }

    /// Another process of the same user binding the port with `SO_REUSEADDR` is refused.
    #[cfg(windows)]
    #[test]
    fn a_reuseaddr_bind_of_the_same_port_fails_on_windows() {
        let listener = bind_loopback_exclusive(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = windows::bind_reuseaddr(port).unwrap_err();
        // WSAEADDRINUSE (10048) or WSAEACCES (10013), nothing else.
        assert!(
            matches!(err.raw_os_error(), Some(10048 | 10013)),
            "{err} ({:?})",
            err.raw_os_error()
        );
    }
}
