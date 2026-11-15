//! The relay's listener: 127.0.0.1 on a port the OS picks.
//!
//! On Windows the socket takes `SO_EXCLUSIVEADDRUSE` before it binds. Without it, another
//! process of the same user (a command in Codex's sandbox) could bind the same address and port
//! with `SO_REUSEADDR` and take agent-browser's connection, and with it the secret in the
//! request line. Std's `TcpListener::bind` cannot set the option before the bind, so the socket
//! is made here and handed to std. Elsewhere std's bind is used: Unix refuses a second bind of a
//! bound port to a different user, and the relay runs only on Windows outside tests.

use std::io;
use std::net::TcpListener;

/// How many connections may wait to be accepted.
#[cfg(windows)]
const BACKLOG: i32 = 16;

/// A listener on 127.0.0.1 at a port the OS picks.
#[cfg(not(windows))]
pub(crate) fn loopback() -> io::Result<TcpListener> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
}

/// A listener on 127.0.0.1 at a port the OS picks, no other socket able to share it.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to Winsock, to set SO_EXCLUSIVEADDRUSE before the relay's bind"
)]
pub(crate) fn loopback() -> io::Result<TcpListener> {
    use std::os::windows::io::{FromRawSocket, RawSocket};

    use windows::Win32::Networking::WinSock::{
        AF_INET, IN_ADDR, IN_ADDR_0, IPPROTO_TCP, SO_EXCLUSIVEADDRUSE, SOCK_STREAM, SOCKADDR,
        SOCKADDR_IN, SOL_SOCKET, WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSADATA,
        WSASocketW, WSAStartup, bind, closesocket, listen, setsockopt,
    };

    let mut data = WSADATA::default();
    // SAFETY: `data` is a live WSADATA the call writes. Winsock counts its starts; this one is
    // never undone, as std never undoes its own.
    if unsafe { WSAStartup(0x0202, &raw mut data) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: plain values; no protocol info. The socket is not inherited by any child.
    let socket = unsafe {
        WSASocketW(
            i32::from(AF_INET.0),
            SOCK_STREAM.0,
            IPPROTO_TCP.0,
            None,
            0,
            WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
        )
    }
    .map_err(io::Error::from)?;
    let close = |error: io::Error| {
        // SAFETY: `socket` is the open socket made above, closed once, here, on failure.
        unsafe { closesocket(socket) };
        error
    };
    let on: i32 = 1;
    // SAFETY: `socket` is open; the value is a live 4-byte BOOL.
    if unsafe {
        setsockopt(
            socket,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            Some(&on.to_ne_bytes()),
        )
    } != 0
    {
        return Err(close(io::Error::last_os_error()));
    }
    let address = SOCKADDR_IN {
        sin_family: AF_INET,
        sin_port: 0,
        sin_addr: IN_ADDR {
            S_un: IN_ADDR_0 {
                S_addr: u32::from_ne_bytes([127, 0, 0, 1]),
            },
        },
        ..SOCKADDR_IN::default()
    };
    let size = i32::try_from(size_of::<SOCKADDR_IN>()).map_err(io::Error::other)?;
    // SAFETY: `address` is a live SOCKADDR_IN of `size` bytes, read during the call only.
    if unsafe { bind(socket, (&raw const address).cast::<SOCKADDR>(), size) } != 0 {
        return Err(close(io::Error::last_os_error()));
    }
    // SAFETY: `socket` is open and bound.
    if unsafe { listen(socket, BACKLOG) } != 0 {
        return Err(close(io::Error::last_os_error()));
    }
    let raw = RawSocket::try_from(socket.0).map_err(|error| close(io::Error::other(error)))?;
    // SAFETY: `raw` is the open listening socket made above, which nothing else owns; the
    // `TcpListener` closes it once.
    Ok(unsafe { TcpListener::from_raw_socket(raw) })
}
