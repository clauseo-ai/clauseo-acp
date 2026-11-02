//! The test network, servers, sink, clock and client helpers the proxy tests share.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use async_io::Async;
use base64::Engine;
use futures::future::BoxFuture;

use crate::proxy::net::Network;
use crate::proxy::{Exchange, Limits, Options, ProxyConfig, ProxyHandle, start_with};
use crate::{Allowlist, Clock, Denial, DenialSink, Reachability, Upstream};

/// How long a client waits for any one read before the test fails.
const CLIENT_WAIT: Duration = Duration::from_secs(10);

/// Names to addresses, and addresses to local servers, with a record of every call.
#[derive(Default)]
pub(crate) struct TestNet {
    /// Successive answers per name; the last one repeats.
    names: Mutex<BTreeMap<String, Vec<Vec<IpAddr>>>>,
    routes: Mutex<BTreeMap<SocketAddr, SocketAddr>>,
    dialled: Mutex<Vec<SocketAddr>>,
    resolved: Mutex<Vec<String>>,
}

impl TestNet {
    /// `host` resolves to `ips`.
    pub(crate) fn name(&self, host: &str, ips: &[&str]) {
        self.answers(host, &[ips]);
    }

    /// `host` resolves to each answer in turn.
    pub(crate) fn answers(&self, host: &str, answers: &[&[&str]]) {
        let answers = answers
            .iter()
            .map(|ips| ips.iter().map(|ip| ip.parse().unwrap()).collect())
            .collect();
        self.names.lock().unwrap().insert(host.to_owned(), answers);
    }

    /// A connect to `from` reaches the server at `to`.
    pub(crate) fn route(&self, from: &str, to: SocketAddr) {
        self.routes
            .lock()
            .unwrap()
            .insert(from.parse().unwrap(), to);
    }

    pub(crate) fn dialled(&self) -> Vec<SocketAddr> {
        self.dialled.lock().unwrap().clone()
    }

    pub(crate) fn resolved(&self) -> Vec<String> {
        self.resolved.lock().unwrap().clone()
    }
}

impl Network for TestNet {
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        self.resolved.lock().unwrap().push(host.to_owned());
        let mut names = self.names.lock().unwrap();
        let answer = names.get_mut(host).and_then(|answers| {
            if answers.len() > 1 {
                Some(answers.remove(0))
            } else {
                answers.first().cloned()
            }
        });
        let result = match answer {
            Some(ips) => Ok(ips
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect()),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such name")),
        };
        Box::pin(async move { result })
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>> {
        self.dialled.lock().unwrap().push(addr);
        let mapped = self.routes.lock().unwrap().get(&addr).copied();
        let target = match mapped {
            Some(to) => Some(to),
            // The upstream proxy in the upstream tests listens on 127.0.0.1 itself.
            None if addr.ip().is_loopback() => Some(addr),
            None => None,
        };
        Box::pin(async move {
            match target {
                Some(to) => Async::<TcpStream>::connect(to).await,
                None => Err(io::Error::from(io::ErrorKind::ConnectionRefused)),
            }
        })
    }
}

/// A sink that keeps everything.
#[derive(Default)]
pub(crate) struct Sink {
    pub(crate) denials: Mutex<Vec<Denial>>,
    pub(crate) reach: Mutex<Vec<Reachability>>,
}

impl DenialSink for Sink {
    fn denied(&self, d: Denial) {
        self.denials.lock().unwrap().push(d);
    }

    fn upstream(&self, r: Reachability) {
        self.reach.lock().unwrap().push(r);
    }
}

/// A clock that moves only when told.
#[derive(Default)]
pub(crate) struct TestClock(Mutex<Duration>);

impl TestClock {
    pub(crate) fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for TestClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
}

/// What a test proxy starts with.
pub(crate) struct Setup {
    pub(crate) allow: Vec<&'static str>,
    pub(crate) silent: Vec<&'static str>,
    pub(crate) upstream: Upstream,
    pub(crate) limits: Limits,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup {
            allow: Vec::new(),
            silent: Vec::new(),
            upstream: Upstream::Direct,
            limits: Limits::default(),
        }
    }
}

/// A running test proxy and everything it reports to.
pub(crate) struct Rig {
    pub(crate) handle: ProxyHandle,
    pub(crate) net: Arc<TestNet>,
    pub(crate) sink: Arc<Sink>,
    pub(crate) clock: Arc<TestClock>,
    pub(crate) records: Arc<Mutex<Vec<Exchange>>>,
}

fn list(entries: &[&str]) -> Allowlist {
    entries.iter().fold(Allowlist::default(), |acc, entry| {
        let one = if entry.starts_with("*.") {
            Allowlist::firm_suffix(entry).unwrap()
        } else {
            Allowlist::exact(entry).unwrap()
        };
        acc.union(one)
    })
}

pub(crate) fn rig(setup: Setup) -> Rig {
    let net = Arc::new(TestNet::default());
    let sink = Arc::new(Sink::default());
    let clock = Arc::new(TestClock::default());
    let records = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&records);
    let config = ProxyConfig {
        allow: list(&setup.allow),
        silent: list(&setup.silent),
        upstream: setup.upstream,
        denials: Arc::clone(&sink) as Arc<dyn DenialSink>,
        record: Some(Arc::new(move |exchange: &Exchange| {
            kept.lock().unwrap().push(exchange.clone());
        })),
    };
    let options = Options {
        clock: Arc::clone(&clock) as Arc<dyn Clock>,
        limits: setup.limits,
    };
    let handle = start_with(config, Arc::clone(&net) as Arc<dyn Network>, options).unwrap();
    Rig {
        handle,
        net,
        sink,
        clock,
        records,
    }
}

pub(crate) fn rig_allowing(allow: &[&'static str]) -> Rig {
    rig(Setup {
        allow: allow.to_vec(),
        ..Setup::default()
    })
}

/// The token in the handle's env.
pub(crate) fn token_of(handle: &ProxyHandle) -> String {
    let (_, value) = handle
        .env()
        .into_iter()
        .find(|(name, _)| name == "HTTPS_PROXY")
        .unwrap();
    let url = url::Url::parse(value.unwrap().to_str().unwrap()).unwrap();
    url.password().unwrap().to_owned()
}

/// A `Proxy-Authorization` line for `token`.
pub(crate) fn auth_line(token: &str) -> String {
    let credential = base64::engine::general_purpose::STANDARD.encode(format!("clauseo:{token}"));
    format!("Proxy-Authorization: Basic {credential}\r\n")
}

impl Rig {
    pub(crate) fn auth(&self) -> String {
        auth_line(&token_of(&self.handle))
    }

    pub(crate) fn agent(&self) -> TcpStream {
        client(self.handle.port())
    }

    pub(crate) fn browser(&self) -> TcpStream {
        client(self.handle.browser_port())
    }

    /// Sends a CONNECT with the token on the agent listener; the stream and the status.
    pub(crate) fn connect(&self, target: &str) -> (TcpStream, u16) {
        let mut stream = self.agent();
        let request = format!(
            "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n{}\r\n",
            self.auth()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let status = status(&read_head(&mut stream));
        (stream, status)
    }

    /// Sends `request` on the agent listener after adding the token; returns the whole answer.
    pub(crate) fn exchange(&self, request: &str) -> String {
        let mut stream = self.agent();
        let with_token = request.replacen("\r\n", &format!("\r\n{}", self.auth()), 1);
        stream.write_all(with_token.as_bytes()).unwrap();
        String::from_utf8_lossy(&read_all(&mut stream)).into_owned()
    }

    pub(crate) fn denials(&self) -> Vec<Denial> {
        self.sink.denials.lock().unwrap().clone()
    }

    pub(crate) fn reach(&self) -> Vec<Reachability> {
        self.sink.reach.lock().unwrap().clone()
    }

    pub(crate) fn records(&self) -> Vec<Exchange> {
        self.records.lock().unwrap().clone()
    }
}

pub(crate) fn client(port: u16) -> TcpStream {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(CLIENT_WAIT)).unwrap();
    stream
}

/// Reads up to and including the blank line that ends a head.
pub(crate) fn read_head(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap() == 0 {
            break;
        }
        buf.push(byte[0]);
    }
    String::from_utf8(buf).unwrap()
}

/// Reads until the peer closes.
pub(crate) fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    buf
}

pub(crate) fn status(head: &str) -> u16 {
    head.split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

/// The `Denial::Host` the proxy sends.
pub(crate) fn host_denial(host: &str, port: u16, why: crate::HostRefusal, shared: bool) -> Denial {
    Denial::Host {
        host: host.to_owned(),
        port,
        why,
        shared_hosting: shared,
    }
}

/// An echo server on 127.0.0.1.
pub(crate) fn echo() -> SocketAddr {
    serve(|mut stream| {
        let mut reader = stream.try_clone().unwrap();
        drop(io::copy(&mut reader, &mut stream));
        drop(stream.shutdown(Shutdown::Write));
    })
}

/// A server on 127.0.0.1 that runs `each` on its own thread per connection.
pub(crate) fn serve(each: impl Fn(TcpStream) + Send + Sync + 'static) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let each = Arc::new(each);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let each = Arc::clone(&each);
            thread::spawn(move || each(stream));
        }
    });
    address
}

/// An HTTP origin (or upstream) that reads one head and its body (by `Content-Length` or to the
/// chunked end), answers `answer`, ends its writes, and sends everything it received, through
/// the proxy's close, on the channel.
pub(crate) fn origin(answer: &'static str) -> (SocketAddr, Receiver<Vec<u8>>) {
    let (sent, received) = mpsc::channel();
    let sent = Mutex::new(sent);
    let address = serve(move |mut stream| {
        stream.set_read_timeout(Some(CLIENT_WAIT)).unwrap();
        let head = read_head(&mut stream);
        let mut got = head.clone().into_bytes();
        let lower = head.to_ascii_lowercase();
        if let Some(length) = lower
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
        {
            let mut body = vec![0u8; length.trim().parse().unwrap()];
            stream.read_exact(&mut body).unwrap();
            got.extend_from_slice(&body);
        } else if lower.contains("transfer-encoding: chunked") {
            let mut byte = [0u8; 1];
            while !got.ends_with(b"0\r\n\r\n") {
                if stream.read(&mut byte).unwrap() == 0 {
                    break;
                }
                got.push(byte[0]);
            }
        }
        stream.write_all(answer.as_bytes()).unwrap();
        drop(stream.shutdown(Shutdown::Write));
        // Whatever else arrives before the proxy closes would be a second request.
        let mut rest = Vec::new();
        drop(stream.read_to_end(&mut rest));
        got.extend_from_slice(&rest);
        sent.lock().unwrap().send(got).unwrap();
    });
    (address, received)
}

/// An upstream proxy that records the head it gets, answers `answer`, and echoes afterwards
/// when it answered 200.
pub(crate) fn upstream(answer: &'static str) -> (SocketAddr, Receiver<String>) {
    let (sent, received) = mpsc::channel();
    let sent = Mutex::new(sent);
    let address = serve(move |mut stream| {
        stream.set_read_timeout(Some(CLIENT_WAIT)).unwrap();
        let head = read_head(&mut stream);
        sent.lock().unwrap().send(head).unwrap();
        stream.write_all(answer.as_bytes()).unwrap();
        if answer.starts_with("HTTP/1.1 200") {
            let mut reader = stream.try_clone().unwrap();
            drop(io::copy(&mut reader, &mut stream));
        }
        drop(stream.shutdown(Shutdown::Write));
    });
    (address, received)
}

/// Writes `data` through a tunnel and reads the same number of bytes back.
pub(crate) fn round_trip(stream: &mut TcpStream, data: &[u8]) -> Vec<u8> {
    stream.write_all(data).unwrap();
    let mut back = vec![0u8; data.len()];
    stream.read_exact(&mut back).unwrap();
    back
}
