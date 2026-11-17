//! A test-only HTTP proxy on `127.0.0.1` that is also the origin of `http://page.test`: the real
//! Chrome test gives it to the guard as the session's egress proxy, so Chrome reaches the page
//! only through it (the guard refuses local hosts, and Chrome uses no proxy bypass). It answers
//! `GET http://page.test/...` itself, refuses `CONNECT` (Chrome's background hosts) and every
//! other host with 403, and logs each request's target. Linux and macOS, with `real_chrome.rs`.
//!
//! [`PageProxy::with_tunnels`] also tunnels `CONNECT <host>:443` for each host it is given to a
//! local TLS server (the certificate rows' `openssl s_server`, `real_chrome/certificate.rs`), and
//! `page.test` then serves [`REDIRECT`] (a 302 to [`REDIRECT_TARGET`]) and [`INTERNAL`] (links to
//! Chrome's own pages).

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

/// The page's heading, which the snapshot must show.
pub(crate) const HEADING: &str = "Clauseo guard regression page";

/// The names the page's links download, each a protected name.
pub(crate) const DOWNLOADS: [&str; 4] = ["CLAUDE.md", "claude.MD", ".mcp.json", "AGENTS.md"];

/// `page.test`'s path that redirects to [`REDIRECT_TARGET`] (with tunnels only).
pub(crate) const REDIRECT: &str = "/to-https";
/// Where [`REDIRECT`] sends the browser: a page whose certificate does not verify.
pub(crate) const REDIRECT_TARGET: &str = "https://bad-self.test/index.html";
/// `page.test`'s path with a link to each of [`INTERNAL_LINKS`] (with tunnels only).
pub(crate) const INTERNAL: &str = "/internal";
/// Chrome's and Edge's own pages, each a link on [`INTERNAL`] with the id `i<n>`.
pub(crate) const INTERNAL_LINKS: [&str; 7] = [
    "chrome://settings/security",
    "chrome://flags",
    "chrome://net-internals/#hsts",
    "chrome://interstitials/ssl",
    "chrome-error://chromewebdata/",
    "edge://settings/privacy",
    "edge://flags",
];

/// The running proxy.
pub(crate) struct PageProxy {
    pub(crate) port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

/// `CONNECT` targets (`host:443`) and the local port each is tunnelled to.
type Tunnels = Arc<BTreeMap<String, u16>>;

impl PageProxy {
    pub(crate) fn start() -> Self {
        Self::with_tunnels(BTreeMap::new())
    }

    /// The proxy, tunnelling `CONNECT <host>` to `127.0.0.1:<port>` for each entry of `tunnels`
    /// (keys `host:443`); every other `CONNECT` is refused as before.
    pub(crate) fn with_tunnels(tunnels: BTreeMap<String, u16>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&log);
        let tunnels: Tunnels = Arc::new(tunnels);
        // Ends with the test process.
        drop(thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&shared);
                let tunnels = Arc::clone(&tunnels);
                drop(thread::spawn(move || drop(serve(stream, &log, &tunnels))));
            }
        }));
        Self { port, log }
    }

    /// Every request's method and target, in order.
    pub(crate) fn log(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn serve(stream: TcpStream, log: &Mutex<Vec<String>>, tunnels: &Tunnels) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header == "\r\n" {
            break;
        }
    }
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    log.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(format!("{method} {target}"));
    if method == "CONNECT"
        && let Some(port) = tunnels.get(target)
    {
        return tunnel(stream, &reader, *port);
    }
    let path = target.strip_prefix("http://page.test");
    let tunnelling = !tunnels.is_empty();
    let (status, headers, body) = match (method, path) {
        ("GET", Some("/")) => ("200 OK", page_headers(), page()),
        ("GET", Some(REDIRECT)) if tunnelling => (
            "302 Found",
            format!("Location: {REDIRECT_TARGET}\r\n"),
            String::new(),
        ),
        ("GET", Some(INTERNAL)) if tunnelling => ("200 OK", page_headers(), internal()),
        ("GET", Some(path)) => match download(path) {
            Some(name) => (
                "200 OK",
                format!(
                    "Content-Type: application/octet-stream\r\nContent-Disposition: attachment; \
                     filename=\"{name}\"\r\n"
                ),
                format!("planted by a web page as {name}\n"),
            ),
            None => ("404 Not Found", String::new(), String::new()),
        },
        _ => ("403 Forbidden", String::new(), String::new()),
    };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// Answers `CONNECT` and joins `stream` to `127.0.0.1:port` both ways until either side ends.
fn tunnel(stream: TcpStream, reader: &BufReader<TcpStream>, port: u16) -> std::io::Result<()> {
    let upstream = TcpStream::connect(("127.0.0.1", port))?;
    let mut client = stream;
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    // Anything the client sent past the request's head (Chrome waits for the answer, so
    // nothing in practice).
    let mut up = upstream.try_clone()?;
    up.write_all(reader.buffer())?;
    let (mut from_client, mut to_client) = (client.try_clone()?, client);
    let mut from_upstream = upstream;
    let forward = thread::spawn(move || {
        drop(std::io::copy(&mut from_client, &mut up));
        drop(up.shutdown(std::net::Shutdown::Write));
    });
    drop(std::io::copy(&mut from_upstream, &mut to_client));
    drop(to_client.shutdown(std::net::Shutdown::Both));
    drop(forward.join());
    Ok(())
}

/// [`INTERNAL`]'s page.
fn internal() -> String {
    let mut links = String::new();
    for (n, url) in INTERNAL_LINKS.iter().enumerate() {
        write!(links, "<li><a id=\"i{n}\" href=\"{url}\">{url}</a></li>").unwrap();
    }
    format!(
        "<!doctype html><html><head><title>Internal links</title></head><body><h1>{HEADING}</h1>\
         <ul>{links}</ul></body></html>"
    )
}

fn page_headers() -> String {
    "Content-Type: text/html; charset=utf-8\r\n".to_owned()
}

/// `/dl/<n>`'s name.
fn download(path: &str) -> Option<&'static str> {
    let n: usize = path.strip_prefix("/dl/")?.parse().ok()?;
    DOWNLOADS.get(n).copied()
}

fn page() -> String {
    let mut links = String::new();
    for (n, name) in DOWNLOADS.iter().enumerate() {
        write!(links, "<li><a id=\"d{n}\" href=\"/dl/{n}\">{name}</a></li>").unwrap();
    }
    format!(
        "<!doctype html><html><head><title>Guard test</title></head><body><h1>{HEADING}</h1>\
         <p>Served through the session's proxy.</p><ul>{links}</ul></body></html>"
    )
}
