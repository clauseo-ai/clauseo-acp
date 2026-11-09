//! The page's only server: an HTTP proxy on 127.0.0.1 (chrome's `--proxy-server`) that answers
//! the absolute-form `GET http://clauseo-proof.test/` with a small page, holding the answer
//! until the owner releases it (so chrome's processes can be inspected mid-load), and every
//! other request with 403. No request leaves the machine.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// The page's address, as chrome asks a proxy for it.
pub(crate) const URL: &str = "http://clauseo-proof.test/";
/// The longest the proxy holds the page if the owner never releases it.
const HOLD_LIMIT: Duration = Duration::from_secs(180);
/// The longest it waits for a request's head.
const READ_LIMIT: Duration = Duration::from_secs(30);
/// The most bytes of a request head it reads.
const HEAD_LIMIT: usize = 64 * 1024;

/// What the proxy saw.
#[derive(Debug, Default)]
struct State {
    /// How many page requests arrived.
    seen: u32,
    /// How many page answers were written in full (write, flush and shutdown all worked).
    delivered: u32,
    /// Whether the owner released the page.
    released: bool,
    /// The requests answered 403, as `METHOD target`.
    refused: Vec<String>,
    /// Connection errors.
    errors: Vec<String>,
}

/// The state and the condition the waits use.
type Shared = Arc<(Mutex<State>, Condvar)>;

/// The running proxy; [`Proxy::stop`] (or drop) ends it.
pub(crate) struct Proxy {
    /// Its port on 127.0.0.1.
    port: u16,
    /// What it saw.
    shared: Shared,
    /// Set to end the accept loop.
    stop: Arc<AtomicBool>,
    /// The accept loop.
    accept: Option<JoinHandle<()>>,
}

impl Proxy {
    /// Listens on `127.0.0.1:0` and serves on threads of its own.
    pub(crate) fn start() -> io::Result<Proxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let shared: Shared = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let accept = {
            let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
            thread::spawn(move || accept_loop(&listener, &shared, &stop))
        };
        Ok(Proxy {
            port,
            shared,
            stop,
            accept: Some(accept),
        })
    }

    /// Its port.
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Waits at most `timeout` for the first page request; whether one arrived.
    pub(crate) fn wait_seen(&self, timeout: Duration) -> bool {
        let (lock, ready) = &*self.shared;
        let state = lock.lock().unwrap();
        let (state, _timed_out) = ready
            .wait_timeout_while(state, timeout, |s| s.seen == 0)
            .unwrap();
        state.seen > 0
    }

    /// How many page requests arrived.
    pub(crate) fn seen(&self) -> u32 {
        self.shared.0.lock().unwrap().seen
    }

    /// How many page answers it wrote in full.
    pub(crate) fn delivered(&self) -> u32 {
        self.shared.0.lock().unwrap().delivered
    }

    /// The requests it refused and the connection errors, for the evidence.
    pub(crate) fn refused_and_errors(&self) -> (Vec<String>, Vec<String>) {
        let state = self.shared.0.lock().unwrap();
        (state.refused.clone(), state.errors.clone())
    }

    /// Lets the held page answers go.
    pub(crate) fn release(&self) {
        let (lock, ready) = &*self.shared;
        lock.lock().unwrap().released = true;
        ready.notify_all();
    }

    /// Releases the page, ends the accept loop and waits for it.
    pub(crate) fn stop(&mut self) -> String {
        self.release();
        self.stop.store(true, Ordering::SeqCst);
        let Some(accept) = self.accept.take() else {
            return "already stopped".to_owned();
        };
        // A connection wakes the blocked accept so it sees the flag.
        if let Err(e) = TcpStream::connect(("127.0.0.1", self.port)) {
            return format!("could not wake the accept loop ({e}); left it blocked");
        }
        match accept.join() {
            Ok(()) => "stopped".to_owned(),
            Err(_panic) => "the accept loop had panicked".to_owned(),
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        let _how: String = self.stop();
    }
}

/// Accepts until `stop`, one thread per connection.
fn accept_loop(listener: &TcpListener, shared: &Shared, stop: &AtomicBool) {
    for conn in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(conn) = conn else { continue };
        let shared = Arc::clone(shared);
        thread::spawn(move || {
            if let Err(e) = serve(conn, &shared) {
                shared.0.lock().unwrap().errors.push(e.to_string());
            }
        });
    }
}

/// Answers one connection: the page (after the hold) or 403.
fn serve(mut conn: TcpStream, shared: &Shared) -> io::Result<()> {
    conn.set_read_timeout(Some(READ_LIMIT))?;
    let head = read_head(&mut conn)?;
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let is_page = method == "GET" && target == URL;
    let reply = if is_page {
        hold(shared);
        page()
    } else {
        let mut state = shared.0.lock().unwrap();
        state.refused.push(format!("{method} {target}"));
        forbidden()
    };
    conn.write_all(reply.as_bytes())?;
    conn.flush()?;
    conn.shutdown(Shutdown::Write)?;
    if is_page {
        shared.0.lock().unwrap().delivered += 1;
    }
    Ok(())
}

/// Counts a page request and waits for the release (at most [`HOLD_LIMIT`]).
fn hold(shared: &Shared) {
    let (lock, ready) = &**shared;
    let mut state = lock.lock().unwrap();
    state.seen += 1;
    ready.notify_all();
    let (_state, _timed_out) = ready
        .wait_timeout_while(state, HOLD_LIMIT, |s| !s.released)
        .unwrap();
}

/// The request's head, up to the blank line.
fn read_head(conn: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < HEAD_LIMIT {
        let read = conn.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// The page: a coloured box and a line of text.
fn page() -> String {
    let body = "<!doctype html><html><head><title>Clauseo lowbox proof</title></head>\
                <body style=\"margin:0;background:#ffffff\">\
                <div style=\"width:640px;height:360px;background:#1565c0;color:#ffffff;\
                font:bold 40px sans-serif;display:flex;align-items:center;\
                justify-content:center\">Clauseo lowbox proof</div></body></html>";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The answer to everything else.
fn forbidden() -> String {
    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
}
