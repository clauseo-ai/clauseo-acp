//! Test support: a local registry on loopback that misbehaves on demand, an optional
//! TLS-inspecting proxy in front of it, test resolvers, temp folders and tiny npm tarballs.
//!
//! The registry speaks just enough HTTP/1.1 for ureq: `GET`, `Range: bytes=N-`,
//! `Content-Length`, `Content-Range` and `Connection: close`, one thread per connection. Nothing
//! here sleeps: a stalled response waits on a condvar for [`TestRegistry::release`].

mod pack;
mod tls;
mod trace;

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

pub use pack::{npm_tarball, sha512_integrity};
pub use trace::{instant, millis_since, trace};

use crate::TestResolver;

/// How a route answers its next request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// The bytes (or the range asked for), whole.
    Normal,
    /// Headers for the whole length, then the connection closes after this many body bytes.
    CutAfter(usize),
    /// The bytes with one byte, in the middle, flipped.
    Tamper,
    /// This status, with a short plain-text body.
    Status(u16),
    /// A `text/html` page starting `<!DOCTYPE html>`, as a proxy's block page would.
    Html,
    /// Headers and the first half of the body, then a wait for [`TestRegistry::release`]
    /// before the rest.
    Stall,
    /// The bytes with no `Content-Length`: the body ends when the connection closes.
    NoLength,
}

/// One request the registry saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `GET`, or `CONNECT` for a proxy tunnel.
    pub method: String,
    /// The path (`host:port` for a `CONNECT`).
    pub path: String,
    /// The `Range` header, when sent.
    pub range: Option<String>,
    /// The `Host` header, when sent.
    pub host: Option<String>,
    /// Whether it came inside the proxy's TLS tunnel.
    pub tunnelled: bool,
}

/// A registry on `127.0.0.1:<port>`. Dropping it stops it and releases every stall.
#[derive(Debug)]
pub struct TestRegistry {
    addr: SocketAddr,
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    ca: Option<tls::TestCa>,
}

#[derive(Debug, Default)]
struct State {
    routes: HashMap<String, Route>,
    requests: Vec<Request>,
    /// Bumped by each release; a stall waits for it to move past the value it saw.
    releases: u64,
    /// Stalls that have sent their first half.
    stalled: usize,
    stopped: bool,
}

#[derive(Debug)]
struct Route {
    bytes: Arc<[u8]>,
    queue: VecDeque<Behaviour>,
}

impl TestRegistry {
    /// A plain-HTTP registry with no routes.
    pub fn start() -> io::Result<TestRegistry> {
        TestRegistry::start_with(None)
    }

    /// A registry that is also a TLS-inspecting proxy: a `CONNECT host:port` gets a 200, then
    /// TLS with a leaf certificate for `host` signed by a CA made now ([`Self::ca_pem`]), then
    /// the same routes inside the tunnel.
    pub fn start_tls_proxy() -> io::Result<TestRegistry> {
        TestRegistry::start_with(Some(tls::TestCa::new()?))
    }

    fn start_with(ca: Option<tls::TestCa>) -> io::Result<TestRegistry> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            ca,
        });
        let accepting = Arc::clone(&shared);
        trace(format_args!("registry {addr}: listening"));
        std::thread::Builder::new()
            .name("test-registry".to_owned())
            .spawn(move || accept(&listener, &accepting))?;
        Ok(TestRegistry { addr, shared })
    }

    /// Its address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>{path}`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// `http://127.0.0.1:<port>`, for a client's proxy setting.
    pub fn proxy_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The test CA's certificate as PEM, for a TLS-inspecting registry.
    pub fn ca_pem(&self) -> Option<String> {
        self.shared.ca.as_ref().map(tls::TestCa::pem)
    }

    /// Serves `bytes` at `path` (replacing any route there, and its queue).
    pub fn route(&self, path: &str, bytes: Vec<u8>) {
        self.shared.lock().routes.insert(
            path.to_owned(),
            Route {
                bytes: Arc::from(bytes),
                queue: VecDeque::new(),
            },
        );
    }

    /// Queues `behaviour` for the next request to `path` that the queue reaches; once the queue
    /// is empty, requests are served [`Behaviour::Normal`]. A path with no route is ignored.
    pub fn then(&self, path: &str, behaviour: Behaviour) {
        if let Some(route) = self.shared.lock().routes.get_mut(path) {
            route.queue.push_back(behaviour);
        }
    }

    /// Every request so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.shared.lock().requests.clone()
    }

    /// Lets every stall waiting now send the rest of its body.
    pub fn release(&self) {
        self.shared.lock().releases += 1;
        self.shared.changed.notify_all();
    }

    /// Blocks until `n` stalls in all have sent their first half.
    pub fn wait_for_stalls(&self, n: usize) {
        let mut state = self.shared.lock();
        while state.stalled < n && !state.stopped {
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Drop for TestRegistry {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.stopped = true;
            state.releases += 1;
        }
        self.shared.changed.notify_all();
        // Wakes the accept loop so it sees the stop.
        drop(TcpStream::connect(self.addr));
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.lock().stopped {
            return;
        }
        let Ok(stream) = stream else { continue };
        let peer = stream
            .peer_addr()
            .map_or_else(|_| "?".to_owned(), |a| a.to_string());
        trace(format_args!("registry: accepted a connection from {peer}"));
        let shared = Arc::clone(shared);
        let spawned = std::thread::Builder::new()
            .name("test-registry-conn".to_owned())
            .spawn(move || connection(stream, &shared));
        drop(spawned);
    }
}

/// One connection: a request, or a `CONNECT` and then one request inside the tunnel.
fn connection(mut stream: TcpStream, shared: &Shared) {
    let Ok(Some(head)) = read_head(&mut stream) else {
        // Not HTTP (a TLS hello sent to plain HTTP, say): a plain-text 400, as a server would.
        let reply = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        drop(stream.write_all(reply));
        return;
    };
    let Some(request) = parse_head(&head, false) else {
        return;
    };
    if request.method != "CONNECT" {
        let start = instant();
        let served = respond(&mut stream, shared, &request);
        let ms = millis_since(start);
        trace(format_args!(
            "registry: {} {} served in {ms} ms: {served:?}",
            request.method, request.path
        ));
        return;
    }
    shared.lock().requests.push(request.clone());
    let Some(ca) = shared.ca.as_ref() else {
        drop(write_status(
            &mut stream,
            405,
            "text/plain",
            b"no proxy here",
        ));
        return;
    };
    if stream
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .is_err()
    {
        return;
    }
    let host = request
        .path
        .rsplit_once(':')
        .map_or(request.path.as_str(), |(host, _)| host);
    let Ok(mut tunnel) = ca.accept(host, stream) else {
        return;
    };
    let Ok(Some(head)) = read_head(&mut tunnel) else {
        return;
    };
    let Some(inner) = parse_head(&head, true) else {
        return;
    };
    let start = instant();
    let served = respond(&mut tunnel, shared, &inner);
    let ms = millis_since(start);
    trace(format_args!(
        "registry: {} {} in the tunnel served in {ms} ms: {served:?}",
        inner.method, inner.path
    ));
    tunnel.conn.send_close_notify();
    drop(tunnel.flush());
}

/// The request head up to its blank line, or `None` when the first byte is not a method's.
fn read_head(stream: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        head.extend_from_slice(buf.get(..n).unwrap_or_default());
        if !head.first().is_some_and(u8::is_ascii_uppercase) {
            return Ok(None);
        }
        if head.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(Some(head));
        }
        if head.len() > 16 * 1024 {
            return Ok(None);
        }
    }
}

fn parse_head(head: &[u8], tunnelled: bool) -> Option<Request> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut range = None;
    let mut host = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        if name.eq_ignore_ascii_case("range") {
            range = Some(value);
        } else if name.eq_ignore_ascii_case("host") {
            host = Some(value);
        }
    }
    Some(Request {
        method,
        path,
        range,
        host,
        tunnelled,
    })
}

/// Records `request` and answers it from its route.
fn respond(stream: &mut impl Write, shared: &Shared, request: &Request) -> io::Result<()> {
    let (bytes, behaviour) = {
        let mut state = shared.lock();
        state.requests.push(request.clone());
        let Some(route) = state.routes.get_mut(&request.path) else {
            drop(state);
            return write_status(stream, 404, "text/plain", b"not found");
        };
        (
            Arc::clone(&route.bytes),
            route.queue.pop_front().unwrap_or(Behaviour::Normal),
        )
    };
    trace(format_args!(
        "registry: {} {} range {:?}: {behaviour:?}",
        request.method, request.path, request.range
    ));
    let len = bytes.len();
    let start = request
        .range
        .as_deref()
        .and_then(|range| range.strip_prefix("bytes="))
        .and_then(|range| range.strip_suffix('-'))
        .and_then(|start| start.parse::<usize>().ok());
    let (status, body, range) = match start {
        Some(start) if start >= len => {
            let header = format!("Content-Range: bytes */{len}\r\n");
            return write_response(stream, 416, "text/plain", &header, b"", 0);
        }
        Some(start) => {
            let range = format!("Content-Range: bytes {start}-{}/{len}\r\n", len - 1);
            (206, bytes.get(start..).unwrap_or_default(), range)
        }
        None => (200, &bytes[..], String::new()),
    };
    let kind = "application/octet-stream";
    match behaviour {
        Behaviour::Normal => write_response(stream, status, kind, &range, body, body.len()),
        Behaviour::CutAfter(n) => {
            let sent = body.get(..n.min(body.len())).unwrap_or_default();
            write_response(stream, status, kind, &range, sent, body.len())
        }
        Behaviour::Tamper => {
            let mut tampered = body.to_vec();
            if let Some(byte) = tampered.get_mut(body.len() / 2) {
                *byte ^= 0xff;
            }
            write_response(stream, status, kind, &range, &tampered, body.len())
        }
        Behaviour::Status(code) => write_status(stream, code, "text/plain", b"refused"),
        Behaviour::Html => {
            let page = b"<!DOCTYPE html><html><body>Blocked by policy</body></html>";
            write_status(stream, 200, "text/html; charset=utf-8", page)
        }
        Behaviour::NoLength => {
            let head = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: {kind}\r\n{range}Connection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(body)?;
            stream.flush()
        }
        Behaviour::Stall => {
            let (first, rest) = body.split_at(body.len() / 2);
            write_response(stream, status, kind, &range, first, body.len())?;
            stream.flush()?;
            stall(shared);
            stream.write_all(rest)?;
            stream.flush()
        }
    }
}

/// Counts a stall, then waits for the next release (or the registry's drop).
fn stall(shared: &Shared) {
    let mut state = shared.lock();
    let seen = state.releases;
    state.stalled += 1;
    shared.changed.notify_all();
    while state.releases == seen {
        state = shared
            .changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

fn write_status(stream: &mut impl Write, code: u16, kind: &str, body: &[u8]) -> io::Result<()> {
    write_response(stream, code, kind, "", body, body.len())
}

/// Headers claiming `length` body bytes, then `body` (which may be shorter).
fn write_response(
    stream: &mut impl Write,
    code: u16,
    kind: &str,
    extra: &str,
    body: &[u8],
    length: usize,
) -> io::Result<()> {
    let reason = match code {
        200 => "OK",
        206 => "Partial Content",
        403 => "Forbidden",
        404 => "Not Found",
        407 => "Proxy Authentication Required",
        416 => "Range Not Satisfiable",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {length}\r\n\
         {extra}Connection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// The host names a test resolver was asked for, in order.
#[derive(Debug, Clone, Default)]
pub struct Lookups(Arc<Mutex<Vec<String>>>);

impl Lookups {
    /// The names asked for so far.
    pub fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// A resolver that sends every name to `addr` (port 0 keeps the URL's port) and records it.
pub fn resolve_all_to(addr: SocketAddr) -> (TestResolver, Lookups) {
    let lookups = Lookups::default();
    let seen = lookups.clone();
    let resolver = TestResolver(Arc::new(move |name: &str| {
        seen.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
        Ok(vec![addr])
    }));
    (resolver, lookups)
}

/// A resolver for which every lookup fails, as with no network.
pub fn offline_resolver() -> TestResolver {
    TestResolver(Arc::new(|name: &str| {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no address for {name}"),
        ))
    }))
}

/// A folder under the system temp folder, removed with everything in it on drop.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new empty folder whose name holds `label`, this process's id and a counter.
    pub fn new(label: &str) -> io::Result<TempDir> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("clauseo-agents-{label}-{}-{n}", std::process::id()));
        crate::write::remove_dir_all(&path)?;
        crate::write::create_dir_all(&path)?;
        Ok(TempDir(path))
    }

    /// Its path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        crate::write::remove_dir_all(&self.0).unwrap_or_default();
    }
}

/// An install for tests: `node` runs `script` (the sign-in tests' fake CLI), with
/// [`crate::AgentsConfig::ready_for_tests`].
pub fn installed(
    node: PathBuf,
    script: PathBuf,
    folder: PathBuf,
    version: &str,
) -> crate::Installed {
    crate::Installed {
        node,
        script,
        folder,
        version: version.to_owned(),
    }
}

/// A `Failed` install of `kind` for tests, with no space figures and an empty log.
pub fn failed(agent: &clauseo_protocol::AgentId, kind: crate::FailureKind) -> crate::InstallState {
    crate::InstallState::Failed(crate::Failure {
        kind,
        need_mb: None,
        free_mb: None,
        log: crate::InstallLog::new(agent),
    })
}

/// Writes `bytes` to `path`, making its folder, replacing any file there.
pub fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        crate::write::create_dir_all(parent)?;
    }
    let mut file = crate::write::open_truncate(path)?;
    file.write_all(bytes)?;
    file.flush()
}

/// Removes the file at `path`; a missing file is fine.
pub fn remove_file(path: &Path) -> io::Result<()> {
    crate::write::remove_file(path)
}

/// Makes a hard link at `link` to `original`: one file under two names.
pub fn hard_link(original: &Path, link: &Path) -> io::Result<()> {
    crate::write::hard_link(original, link)
}

/// Makes a symbolic link at `link` to `target` (Unix).
#[cfg(unix)]
pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    crate::write::symlink(target, link)
}
