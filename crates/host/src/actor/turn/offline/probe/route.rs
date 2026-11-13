//! The probe's network side: one try at the agent's service within a cap, as the egress proxy
//! would route it (decision 0039).
//! - Through a proxy, to an `https` service: `CONNECT host:port` with `Host` and no credentials
//!   (as the sandbox's `ask_upstream` sends it), then the answer's head within the cap. 200 goes
//!   on to the client hello; 407 counts as reached; any other status, no head, garbage or a close
//!   is a miss.
//! - Straight to an `https` service: the TCP connection, then the client hello.
//! - The client hello is rustls's own (SNI the service's name, an empty root store), written
//!   once; the first 5 bytes back decide: a handshake record (`0x16`, version `0x03`) is
//!   reached, anything else is not. The server's bytes are never fed to rustls and no
//!   certificate is checked, so a firm's TLS-inspecting proxy counts as reached; nothing but the
//!   hello is ever sent.
//! - A plain `http` service: the TCP connection only (to the proxy or the host).

use std::io::{ErrorKind, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs as _};
use std::sync::Arc;
use std::time::Duration;

use clauseo_sandbox::proxy::{Route, Upstream, upstream_for};
use url::{Host, Url};

use super::{Reach, ReachClass, Via};
use crate::clock::{Clock as _, SystemClock};

/// The most of a `CONNECT` answer's head read.
const HEAD_MAX: usize = 8 * 1024;

/// A TLS handshake record's content type.
const HANDSHAKE: u8 = 0x16;

/// The real probe of `url`: its route, then [`probe_route`].
pub(super) fn probe(url: &Url, cap: Duration) -> Reach {
    probe_route(upstream_for(&Upstream::System, url).ok(), url, cap)
}

/// One try at `url` by `route` (`None`: no route could be read, so straight to the host), all
/// within `cap`.
pub(super) fn probe_route(route: Option<Route>, url: &Url, cap: Duration) -> Reach {
    let clock = SystemClock;
    let started = clock.now().mono;
    let deadline = Deadline {
        clock,
        end: started.saturating_add(cap),
    };
    let via = match route {
        Some(Route::Http { .. }) => Via::Proxy,
        Some(Route::Direct) | None => Via::Direct,
    };
    let (class, status) = attempt(route, url, &deadline);
    let took = clock.now().mono.saturating_sub(started).as_millis();
    Reach {
        class,
        via,
        status,
        ms: u32::try_from(took).unwrap_or(u32::MAX),
    }
}

/// The time left of the cap.
struct Deadline {
    clock: SystemClock,
    end: Duration,
}

impl Deadline {
    /// What is left, `None` once nothing is.
    fn left(&self) -> Option<Duration> {
        Some(self.end.saturating_sub(self.clock.now().mono)).filter(|left| !left.is_zero())
    }
}

fn attempt(route: Option<Route>, url: &Url, deadline: &Deadline) -> (ReachClass, u16) {
    let (Some(host), Some(port)) = (bare_host(url), url.port_or_known_default()) else {
        return (ReachClass::NoService, 0);
    };
    let tls = matches!(url.scheme(), "https" | "wss");
    match route {
        Some(Route::Http {
            host: proxy,
            port: proxy_port,
        }) => {
            let mut stream = match connect(&proxy, proxy_port, deadline) {
                Ok(stream) => stream,
                Err(class) => return (class, 0),
            };
            if !tls {
                return (ReachClass::Plain, 0);
            }
            let authority = url.host_str().map(|name| format!("{name}:{port}"));
            match authority.map(|authority| tunnel(&mut stream, &authority, deadline)) {
                Some(Some(200)) => (hello(&mut stream, &host, deadline), 200),
                Some(Some(407)) => (ReachClass::ProxyAuth, 407),
                Some(Some(status)) => (ReachClass::TunnelRefused, status),
                Some(None) | None => (ReachClass::TunnelSilent, 0),
            }
        }
        Some(Route::Direct) | None => {
            let mut stream = match connect(&host, port, deadline) {
                Ok(stream) => stream,
                Err(class) => return (class, 0),
            };
            if tls {
                (hello(&mut stream, &host, deadline), 0)
            } else {
                (ReachClass::Plain, 0)
            }
        }
    }
}

/// `url`'s host as a lookup and a TLS name take it: a name, or an IP literal without brackets.
fn bare_host(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(name) => Some(name.to_owned()),
        Host::Ipv4(ip) => Some(ip.to_string()),
        Host::Ipv6(ip) => Some(ip.to_string()),
    }
}

/// A TCP connection to `host:port` within the deadline. The name is resolved on a thread of
/// its own, left to end there when it runs past the cap.
fn connect(host: &str, port: u16, deadline: &Deadline) -> Result<TcpStream, ReachClass> {
    let left = deadline.left().ok_or(ReachClass::ConnectTimeout)?;
    let addrs = resolve(host, port, left)?;
    let mut class = ReachClass::Refused;
    for addr in addrs {
        let left = deadline.left().ok_or(ReachClass::ConnectTimeout)?;
        match TcpStream::connect_timeout(&addr, left) {
            Ok(stream) => return Ok(stream),
            Err(error) if error.kind() == ErrorKind::TimedOut => {
                class = ReachClass::ConnectTimeout;
            }
            Err(_) => {}
        }
    }
    Err(class)
}

fn resolve(host: &str, port: u16, left: Duration) -> Result<Vec<SocketAddr>, ReachClass> {
    let (send, got) = std::sync::mpsc::sync_channel(1);
    let host = host.to_owned();
    std::thread::Builder::new()
        .name("clauseo-offline-lookup".to_owned())
        .spawn(move || {
            let addrs = (host.as_str(), port)
                .to_socket_addrs()
                .map(Iterator::collect::<Vec<_>>);
            // The probe may have stopped waiting.
            let _sent = send.send(addrs);
        })
        .map_err(|_spawn| ReachClass::NoName)?;
    match got.recv_timeout(left) {
        Ok(Ok(addrs)) if !addrs.is_empty() => Ok(addrs),
        Ok(_) => Err(ReachClass::NoName),
        Err(_) => Err(ReachClass::ConnectTimeout),
    }
}

/// Sends `CONNECT authority` and reads the answer's head: its status, `None` when no head came
/// in time, the proxy closed, or the head is not HTTP.
fn tunnel(stream: &mut TcpStream, authority: &str, deadline: &Deadline) -> Option<u16> {
    let ask = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    write_all(stream, ask.as_bytes(), deadline)?;
    let mut head = Vec::new();
    let mut buf = [0_u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        if head.len() >= HEAD_MAX {
            break;
        }
        let read = read_some(stream, &mut buf, deadline)?;
        head.extend_from_slice(buf.get(..read)?);
    }
    status_of(&head)
}

/// The status of an HTTP answer's head (`HTTP/1.x NNN ...`).
fn status_of(head: &[u8]) -> Option<u16> {
    let line = head.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split_ascii_whitespace();
    words
        .next()
        .filter(|version| version.starts_with("HTTP/1."))?;
    let status = words.next()?;
    (status.len() == 3).then_some(())?;
    status.parse().ok()
}

/// Writes a client hello for `host` and reads the first record's header back.
fn hello(stream: &mut TcpStream, host: &str, deadline: &Deadline) -> ReachClass {
    let Some(hello) = client_hello(host) else {
        return ReachClass::TlsOther;
    };
    if write_all(stream, &hello, deadline).is_none() {
        return ReachClass::TlsSilent;
    }
    let mut header = [0_u8; 5];
    let mut got = 0;
    while got < header.len() {
        let Some(rest) = header.get_mut(got..) else {
            break;
        };
        match read_once(stream, rest, deadline) {
            Read::Got(0) | Read::Closed => return ReachClass::TlsOther,
            Read::Got(read) => got += read,
            Read::Silent => return ReachClass::TlsSilent,
        }
    }
    let [kind, major, ..] = header;
    if kind == HANDSHAKE && major == 0x03 {
        ReachClass::Answered
    } else {
        ReachClass::TlsOther
    }
}

/// rustls's client hello for `host` (SNI for a name, none for an IP literal), as bytes.
pub(super) fn client_hello(host: &str) -> Option<Vec<u8>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_owned()).ok()?;
    let mut connection = rustls::ClientConnection::new(Arc::new(config), name).ok()?;
    let mut bytes = Vec::new();
    while connection.wants_write() {
        connection.write_tls(&mut bytes).ok()?;
    }
    Some(bytes)
}

/// One read's outcome.
enum Read {
    Got(usize),
    Closed,
    Silent,
}

fn read_once(stream: &mut TcpStream, buf: &mut [u8], deadline: &Deadline) -> Read {
    loop {
        let Some(left) = deadline.left() else {
            return Read::Silent;
        };
        if stream.set_read_timeout(Some(left)).is_err() {
            return Read::Closed;
        }
        match stream.read(buf) {
            Ok(read) => return Read::Got(read),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            // A read timeout is `WouldBlock` on Unix and `TimedOut` on Windows.
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Read::Silent;
            }
            Err(_) => return Read::Closed,
        }
    }
}

/// Some bytes, `None` on a close, an error or the end of the cap.
fn read_some(stream: &mut TcpStream, buf: &mut [u8], deadline: &Deadline) -> Option<usize> {
    match read_once(stream, buf, deadline) {
        Read::Got(0) | Read::Closed | Read::Silent => None,
        Read::Got(read) => Some(read),
    }
}

fn write_all(stream: &mut TcpStream, bytes: &[u8], deadline: &Deadline) -> Option<()> {
    stream.set_write_timeout(Some(deadline.left()?)).ok()?;
    stream.write_all(bytes).ok()
}
