//! A test-only HTTP proxy on `127.0.0.1` that is also the origin of `http://page.test`: the real
//! Chrome test gives it to the guard as the session's egress proxy, so Chrome reaches the page
//! only through it (the guard refuses local hosts, and Chrome uses no proxy bypass). It answers
//! `GET http://page.test/...` itself, refuses `CONNECT` (Chrome's background hosts) and every
//! other host with 403, and logs each request's target. Linux only, with `real_chrome.rs`.

#![cfg(target_os = "linux")]

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

/// The page's heading, which the snapshot must show.
pub(crate) const HEADING: &str = "Clauseo guard regression page";

/// The names the page's links download, each a protected name.
pub(crate) const DOWNLOADS: [&str; 4] = ["CLAUDE.md", "claude.MD", ".mcp.json", "AGENTS.md"];

/// The running proxy.
pub(crate) struct PageProxy {
    pub(crate) port: u16,
    log: Arc<Mutex<Vec<String>>>,
}

impl PageProxy {
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&log);
        // Ends with the test process.
        drop(thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&shared);
                drop(thread::spawn(move || drop(serve(stream, &log))));
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

fn serve(stream: TcpStream, log: &Mutex<Vec<String>>) -> std::io::Result<()> {
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
    let path = target.strip_prefix("http://page.test");
    let (status, headers, body) = match (method, path) {
        ("GET", Some("/")) => ("200 OK", page_headers(), page()),
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
