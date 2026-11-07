//! The agent's side of the VM rows: each step tries one network reach and its bit in the exit
//! code says whether it got through. Run as the pool user under the (B) token by `launch.rs`;
//! the restricted token can't write a file the desktop user reads, so the exit code carries
//! the results.
//!
//! Steps, one argument each:
//! - `connect:<ip>:<port>` (an IPv6 address in brackets): a TCP connect;
//! - `http:<host>`: the host resolved, then a plain `GET /` to port 80, direct;
//! - `udp53:<ip>`: a DNS query sent straight to `<ip>:53`, answered or not;
//! - `resolve:<host>`: a name lookup, which Windows makes through the DNS Client service;
//! - `proxy:<port>:<base64 credential>`: `GET http://example.com/` through the S16 proxy on
//!   `127.0.0.1:<port>`, a `200` counting;
//! - `pair:<port>`: a listener on `127.0.0.1:<port>` and a connect to it from this process;
//! - `share:<path>`: a folder listed over SMB (`\\localhost\C$`, a guest share).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

/// Set in every exit code the probe gives, so a code without it (an NTSTATUS from a probe
/// that never started) isn't read as results.
pub(crate) const RAN: u32 = 1 << 30;
/// The most steps one probe takes.
pub(crate) const MAX_STEPS: usize = 30;

/// How long one reach may take.
const WAIT: Duration = Duration::from_secs(4);

/// Runs `steps`; the exit code: [`RAN`] and bit `i` for each step `i` that got through.
pub(crate) fn run(steps: &[String]) -> u32 {
    steps
        .iter()
        .take(MAX_STEPS)
        .enumerate()
        .filter(|(_, step)| reached(step))
        .fold(RAN, |code, (i, _)| code | (1 << i))
}

/// Whether bit `i` of a probe's exit `code` says step `i` got through; `None` when the code
/// isn't a probe's results.
pub(crate) fn bit(code: u32, i: usize) -> Option<bool> {
    (code & 0xC000_0000 == RAN).then_some(code & (1 << i) != 0)
}

/// Whether `step` got through.
fn reached(step: &str) -> bool {
    let (kind, rest) = step.split_once(':').unwrap_or((step, ""));
    match kind {
        "connect" => rest
            .parse::<SocketAddr>()
            .is_ok_and(|to| TcpStream::connect_timeout(&to, WAIT).is_ok()),
        "http" => http(rest),
        "udp53" => udp53(rest),
        "resolve" => (rest, 443)
            .to_socket_addrs()
            .is_ok_and(|mut a| a.next().is_some()),
        "proxy" => rest
            .split_once(':')
            .is_some_and(|(port, credential)| proxy(port, credential)),
        "pair" => rest.parse::<u16>().is_ok_and(pair),
        "share" => std::fs::read_dir(rest).is_ok(),
        _ => false,
    }
}

/// The status line's code of what `stream` answers to `request`.
fn status(mut stream: TcpStream, request: &str) -> Option<u16> {
    stream.set_read_timeout(Some(WAIT)).ok()?;
    stream.write_all(request.as_bytes()).ok()?;
    let mut head = [0u8; 64];
    let read = stream.read(&mut head).ok()?;
    let line = std::str::from_utf8(head.get(..read)?).ok()?;
    line.split(' ').nth(1)?.parse().ok()
}

fn http(host: &str) -> bool {
    let Some(to) = (host, 80).to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return false;
    };
    let Ok(stream) = TcpStream::connect_timeout(&to, WAIT) else {
        return false;
    };
    let request = format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    status(stream, &request).is_some()
}

fn udp53(ip: &str) -> bool {
    let Ok(to) = format!("{ip}:53").parse::<SocketAddr>() else {
        return false;
    };
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return false;
    };
    // A query for example.com, type A: id 0x5315, recursion desired.
    let query = [
        0x53, 0x15, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 7, b'e', b'x',
        b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0x00, 0x01, 0x00, 0x01,
    ];
    let mut answer = [0u8; 512];
    socket.set_read_timeout(Some(WAIT)).is_ok()
        && socket.send_to(&query, to).is_ok()
        && socket.recv_from(&mut answer).is_ok()
}

fn proxy(port: &str, credential: &str) -> bool {
    let Ok(to) = format!("127.0.0.1:{port}").parse::<SocketAddr>() else {
        return false;
    };
    let Ok(stream) = TcpStream::connect_timeout(&to, WAIT) else {
        return false;
    };
    let request = format!(
        "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nProxy-Authorization: Basic \
         {credential}\r\nConnection: close\r\n\r\n"
    );
    status(stream, &request) == Some(200)
}

fn pair(port: u16) -> bool {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
        return false;
    };
    let to = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&to, WAIT).is_ok() && listener.accept().is_ok()
}
