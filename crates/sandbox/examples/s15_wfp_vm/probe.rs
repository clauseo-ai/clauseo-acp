//! The agent's side of the VM rows: each step tries one network reach, and two bits of the exit
//! code say how it went. Run as the pool user under the (B) token by `launch.rs`; the restricted
//! token can't write a file the desktop user reads, so the exit code carries the results.
//!
//! Steps, one argument each:
//! - `connect:<addr>:<port>` (an IPv6 address in brackets): a TCP connect;
//! - `http:<host>`: the host resolved, then a plain `GET /` to port 80, direct;
//! - `udp53:<ip>`: a DNS query sent straight to `<ip>:53`, answered or not;
//! - `udp:<addr>:<port>`: one datagram sent; a filtered send fails with 10013;
//! - `icmp:<ip>`: one echo through `IcmpSendEcho`;
//! - `resolve:<host>`: a name lookup, which Windows makes through the DNS Client service;
//! - `proxy:<port>:<base64 credential>`: `GET http://example.com/` through the S16 proxy on
//!   `127.0.0.1:<port>`, a `200` counting;
//! - `pair:<port>`: a listener on `127.0.0.1:<port>` and a connect to it from this process;
//! - `accept:<ip>:<port>:<secs>`: a listener, reached when one connection comes in time;
//! - `share:<path>`: a folder listed over SMB or `WebDAV` (`\\host@SSL\DavWWWRoot\x`);
//! - `derived:denyonly:<step>`, `derived:lowbox:<step>`: `<step>` run by a child started under
//!   a token derived from this one (`derived.rs`).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use super::derived;

/// Set in every exit code the probe gives, so a code without it (an NTSTATUS from a probe that
/// never started) isn't read as results.
pub(crate) const RAN: u32 = 1 << 30;
/// The most steps one probe takes: two bits each below [`RAN`].
pub(crate) const MAX_STEPS: usize = 15;

/// How one step went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Got {
    /// It got through.
    Reached,
    /// A filter refused it (`WSAEACCES`).
    Filtered,
    /// It failed otherwise: refused, timed out, unreachable.
    Failed,
    /// It wasn't run.
    NotRun,
}

impl Got {
    fn bits(self) -> u32 {
        match self {
            Got::Reached => 0,
            Got::Filtered => 1,
            Got::Failed => 2,
            Got::NotRun => 3,
        }
    }

    fn from_bits(bits: u32) -> Got {
        match bits & 3 {
            0 => Got::Reached,
            1 => Got::Filtered,
            2 => Got::Failed,
            _ => Got::NotRun,
        }
    }
}

/// `WSAEACCES`: a filter refused the socket call.
const WSAEACCES: i32 = 10013;
/// How long one reach may take.
const WAIT: Duration = Duration::from_secs(4);

/// Runs `steps`; the exit code: [`RAN`] and two bits for each step, the rest not run.
pub(crate) fn run(exe: &std::path::Path, steps: &[String]) -> u32 {
    (0..MAX_STEPS).fold(RAN, |code, i| {
        let got = steps.get(i).map_or(Got::NotRun, |step| reach(exe, step));
        code | (got.bits() << (2 * i))
    })
}

/// How step `i` went, from a probe's exit `code`; `None` when it isn't a probe's results.
pub(crate) fn step(code: u32, i: usize) -> Option<Got> {
    (code & 0xC000_0000 == RAN && i < MAX_STEPS).then(|| Got::from_bits(code >> (2 * i)))
}

/// An io result as a step's.
fn got<T>(result: std::io::Result<T>) -> Got {
    match result {
        Ok(_) => Got::Reached,
        Err(error) if error.raw_os_error() == Some(WSAEACCES) => Got::Filtered,
        Err(_) => Got::Failed,
    }
}

/// How `step` went.
fn reach(exe: &std::path::Path, text: &str) -> Got {
    let (kind, rest) = text.split_once(':').unwrap_or((text, ""));
    match kind {
        "connect" => rest
            .parse::<SocketAddr>()
            .map_or(Got::Failed, |to| got(TcpStream::connect_timeout(&to, WAIT))),
        "http" => http(rest),
        "udp53" => udp53(rest),
        "udp" => rest.parse::<SocketAddr>().map_or(Got::Failed, |to| {
            got(UdpSocket::bind("0.0.0.0:0").and_then(|s| s.send_to(b"s15", to)))
        }),
        "icmp" => derived::icmp(rest),
        "anon" => derived::anonymous(rest),
        "resolve" => got((rest, 443).to_socket_addrs()),
        "proxy" => rest
            .split_once(':')
            .map_or(Got::Failed, |(port, credential)| proxy(port, credential)),
        "pair" => rest.parse::<u16>().map_or(Got::Failed, pair),
        "accept" => accept(rest),
        "share" => got(std::fs::read_dir(rest)),
        "derived" => rest.split_once(':').map_or(Got::Failed, |(how, inner)| {
            // A child that didn't start, or didn't run the probe, is `NotRun`, apart from one that ran
            // and failed to connect.
            derived::run(exe, how, inner)
                .map_or(Got::NotRun, |code| step(code, 0).unwrap_or(Got::NotRun))
        }),
        _ => Got::Failed,
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

fn http(host: &str) -> Got {
    let Some(to) = (host, 80).to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return Got::Failed;
    };
    match TcpStream::connect_timeout(&to, WAIT) {
        Ok(stream) => {
            let request = format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
            if status(stream, &request).is_some() {
                Got::Reached
            } else {
                Got::Failed
            }
        }
        Err(error) => got::<()>(Err(error)),
    }
}

fn udp53(ip: &str) -> Got {
    let Ok(to) = format!("{ip}:53").parse::<SocketAddr>() else {
        return Got::Failed;
    };
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return Got::Failed;
    };
    // A query for example.com, type A: id 0x5315, recursion desired.
    let query = [
        0x53, 0x15, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 7, b'e', b'x',
        b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0x00, 0x01, 0x00, 0x01,
    ];
    let mut answer = [0u8; 512];
    if let Err(error) = socket.send_to(&query, to) {
        return got::<()>(Err(error));
    }
    got(socket
        .set_read_timeout(Some(WAIT))
        .and_then(|()| socket.recv_from(&mut answer)))
}

fn proxy(port: &str, credential: &str) -> Got {
    let Ok(to) = format!("127.0.0.1:{port}").parse::<SocketAddr>() else {
        return Got::Failed;
    };
    match TcpStream::connect_timeout(&to, WAIT) {
        Ok(stream) => {
            let request = format!(
                "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nProxy-Authorization: \
                 Basic {credential}\r\nConnection: close\r\n\r\n"
            );
            if status(stream, &request) == Some(200) {
                Got::Reached
            } else {
                Got::Failed
            }
        }
        Err(error) => got::<()>(Err(error)),
    }
}

fn pair(port: u16) -> Got {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
        return Got::Failed;
    };
    let to = SocketAddr::from(([127, 0, 0, 1], port));
    match TcpStream::connect_timeout(&to, WAIT) {
        Ok(_) => got(listener.accept()),
        Err(error) => got::<()>(Err(error)),
    }
}

fn accept(rest: &str) -> Got {
    let mut parts = rest.rsplitn(3, ':');
    let (Some(secs), Some(port), Some(ip)) = (parts.next(), parts.next(), parts.next()) else {
        return Got::Failed;
    };
    let (Ok(secs), Ok(port)) = (secs.parse::<u64>(), port.parse::<u16>()) else {
        return Got::Failed;
    };
    let Ok(listener) = TcpListener::bind((ip, port)) else {
        return Got::Failed;
    };
    if listener.set_nonblocking(true).is_err() {
        return Got::Failed;
    }
    for _ in 0..secs * 10 {
        if listener.accept().is_ok() {
            return Got::Reached;
        }
        // A tenth of a second between tries; a probe has no clock to inject.
        derived::pause_tenth();
    }
    Got::Failed
}
