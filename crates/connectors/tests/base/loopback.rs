//! `bind_loopback_exclusive` listens on `127.0.0.1` only.

use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use clauseo_connectors::bind_loopback_exclusive;

/// The machine's LAN address: the source address the OS would pick for a documentation address
/// (TEST-NET-1, RFC 5737). A UDP connect only picks a route; it sends nothing.
fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

#[test]
fn the_listener_refuses_a_connection_to_the_lan_address() {
    let listener = bind_loopback_exclusive(0).unwrap();
    let port = listener.local_addr().unwrap().port();
    let Some(lan) = lan_address() else {
        eprintln!("skipped: this machine has no LAN address, so there is nothing to refuse");
        return;
    };
    let result = TcpStream::connect_timeout(&SocketAddr::new(lan, port), Duration::from_secs(2));
    assert!(result.is_err(), "{lan}:{port} accepted a connection");
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(2),
    )
    .unwrap();
}
