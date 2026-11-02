//! The Winsock calls of [`bind_exclusive`]: a socket with `SO_EXCLUSIVEADDRUSE`, bound to
//! `127.0.0.1` and listening, handed to std as a `TcpListener`.

#![expect(
    unsafe_code,
    reason = "Winsock FFI: std does not expose SO_EXCLUSIVEADDRUSE"
)]

use std::io;
use std::net::TcpListener;
use std::os::windows::io::{FromRawSocket as _, RawSocket};
use std::sync::OnceLock;

use windows::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, INVALID_SOCKET, IPPROTO_TCP, SO_EXCLUSIVEADDRUSE, SO_REUSEADDR,
    SOCK_STREAM, SOCKADDR, SOCKADDR_IN, SOCKET, SOCKET_ERROR, SOL_SOCKET, SOMAXCONN,
    WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA, WSAGetLastError, WSASocketW,
    WSAStartup, bind, closesocket, listen, setsockopt,
};

/// Winsock 2.2, started once per process (std starts it too, lazily; the calls are counted).
fn startup() -> io::Result<()> {
    static STARTED: OnceLock<i32> = OnceLock::new();
    let code = *STARTED.get_or_init(|| {
        let mut data = WSADATA::default();
        // SAFETY: `data` is a valid, writable WSADATA for the call's duration.
        unsafe { WSAStartup(0x0202, &raw mut data) }
    });
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

/// The last Winsock error of this thread.
fn last_error() -> io::Error {
    // SAFETY: WSAGetLastError reads this thread's last error and has no preconditions.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() }.0)
}

/// A socket closed on drop unless it was handed to std.
struct Owned(SOCKET);

impl Owned {
    /// The socket, which this value no longer closes.
    fn release(mut self) -> SOCKET {
        std::mem::replace(&mut self.0, INVALID_SOCKET)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0 != INVALID_SOCKET {
            // SAFETY: `self.0` is a socket this value owns and nothing else closes it.
            unsafe { closesocket(self.0) };
        }
    }
}

/// `127.0.0.1:port` with `option` (`SO_EXCLUSIVEADDRUSE` or, in tests, `SO_REUSEADDR`) set
/// before the bind.
fn bind_with(port: u16, option: i32) -> io::Result<TcpListener> {
    startup()?;
    // SAFETY: plain arguments; no protocol info pointer. The flags match std's own sockets.
    let created = unsafe {
        WSASocketW(
            i32::from(AF_INET.0),
            SOCK_STREAM.0,
            IPPROTO_TCP.0,
            None,
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    };
    let Ok(socket) = created else {
        return Err(last_error());
    };
    let socket = Owned(socket);
    let on = 1i32.to_ne_bytes();
    // SAFETY: `socket.0` is a valid socket; the option value is a 4-byte BOOL that outlives the
    // call.
    if unsafe { setsockopt(socket.0, SOL_SOCKET, option, Some(on.as_slice())) } == SOCKET_ERROR {
        return Err(last_error());
    }
    let addr = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: port.to_be(),
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes([127, 0, 0, 1]),
            },
        },
        sin_zero: [0; 8],
    };
    let len = i32::try_from(size_of::<SOCKADDR_IN>()).unwrap_or(i32::MAX);
    // SAFETY: `addr` is a valid SOCKADDR_IN of `len` bytes that outlives the call.
    if unsafe { bind(socket.0, (&raw const addr).cast::<SOCKADDR>(), len) } == SOCKET_ERROR {
        return Err(last_error());
    }
    let backlog = i32::try_from(SOMAXCONN).unwrap_or(i32::MAX);
    // SAFETY: `socket.0` is a valid, bound socket.
    if unsafe { listen(socket.0, backlog) } == SOCKET_ERROR {
        return Err(last_error());
    }
    let raw = RawSocket::try_from(socket.0.0).map_err(io::Error::other)?;
    socket.release();
    // SAFETY: `raw` is a valid, listening socket that nothing else owns: `Owned` released it, so
    // the listener alone closes it.
    Ok(unsafe { TcpListener::from_raw_socket(raw) })
}

/// [`bind_loopback_exclusive`](super::bind_loopback_exclusive) on Windows.
pub(super) fn bind_exclusive(port: u16) -> io::Result<TcpListener> {
    bind_with(port, SO_EXCLUSIVEADDRUSE)
}

/// What another process would try: the same port with `SO_REUSEADDR`.
#[cfg(test)]
pub(super) fn bind_reuseaddr(port: u16) -> io::Result<TcpListener> {
    bind_with(port, SO_REUSEADDR)
}
