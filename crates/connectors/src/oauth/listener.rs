//! Step 7: the one-shot loopback listener. Each connection is read on its own thread with one
//! total deadline of 2 seconds on the monotonic clock for its whole request head, and at most
//! 8 KB of it; only `GET /oauth/callback` reaches the checks, anything else gets 404. A wrong or
//! missing `state` gets 400 and the listener keeps waiting, so no other page can end the
//! sign-in. Then `error`, then `iss`. The callback's connection is handed back open, so the page
//! the browser shows says how the exchange went.
//!
//! The accept loop takes at most 16 connections per tick and looks at the cancel flag and the
//! sign-in timeout every tick, so a connect flood cannot hold it. At most 8 connections are read
//! at once; up to 64 more wait in a queue for a reader, and any beyond that are closed
//! unanswered. A connection at the back of a full queue can wait about 16 s (8 rounds of 8
//! readers at 2 s each) before it is read.
//!
//! Accepted risk: a local process of the same user that floods the port can delay the browser's
//! callback, or get it dropped, and so block the sign-in (such a process can block it in simpler
//! ways too). It cannot exhaust anything: threads stay at 8, the queue at 64 sockets, and the
//! accept loop at 16 accepts per tick.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use zeroize::Zeroizing;

use super::{FAILURE_PAGE, SUCCESS_PAGE, is_error_code};
use crate::cancel::CancelFlag;
use crate::config::Clock;
use crate::error::SignInError;
use crate::secret::Secret;

/// How often the wait looks at the cancel flag, the deadline and new connections.
const TICK: Duration = Duration::from_millis(20);
/// How long one connection may take to send its whole request head, in real time.
const HEAD_DEADLINE: Duration = Duration::from_secs(2);
/// How long writing an answer may take.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// The most request head read from one connection.
const MAX_HEAD: usize = 8 * 1024;
/// Connections read at once; more wait in the queue.
const MAX_READERS: usize = 8;
/// Connections accepted per tick.
const MAX_ACCEPTS_PER_TICK: usize = 16;
/// Accepted connections waiting for a reader; more are closed unanswered.
const MAX_QUEUED: usize = 64;
/// The callback path of the redirect URI.
const CALLBACK_PATH: &str = "/oauth/callback";

/// The listener's load, now and at its peak: reader threads and queued connections. Read by
/// tests through `ConnectorHandle::listener_stats_for_test`.
#[derive(Debug, Default)]
pub(crate) struct ListenerStats {
    pub(crate) readers: AtomicUsize,
    pub(crate) queued: AtomicUsize,
    pub(crate) peak_readers: AtomicUsize,
    pub(crate) peak_queued: AtomicUsize,
}

/// What the callback must carry.
pub(super) struct Expect<'a> {
    pub(super) state: &'a Secret,
    pub(super) issuer: &'a str,
}

/// A `GET /oauth/callback` and its query.
struct Callback {
    stream: TcpStream,
    query: Zeroizing<String>,
}

/// The browser's callback connection, still waiting for its answer.
pub(super) struct Browser {
    stream: TcpStream,
}

impl Browser {
    /// The fixed success page.
    pub(super) fn signed_in(mut self) {
        write_response(&mut self.stream, 200, "OK", Some(SUCCESS_PAGE));
    }

    /// The fixed failure page, with 400.
    pub(super) fn failed(mut self) {
        failed(&mut self.stream);
    }
}

/// Waits for the callback, the deadline or the flag, and returns the code and the browser's
/// connection, unanswered. The listener is closed when this returns.
pub(super) fn wait_for_code(
    listener: TcpListener,
    expect: &Expect<'_>,
    cancel: &CancelFlag,
    clock: &dyn Clock,
    deadline: SystemTime,
    stats: &Arc<ListenerStats>,
) -> Result<(Secret, Browser), SignInError> {
    let (sender, receiver) = mpsc::channel::<Callback>();
    let mut queue: VecDeque<TcpStream> = VecDeque::new();
    loop {
        if cancel.is_cancelled() {
            return Err(SignInError::Cancelled);
        }
        if clock.now() >= deadline {
            return Err(SignInError::TimedOut);
        }
        for _ in 0..MAX_ACCEPTS_PER_TICK {
            let Ok((stream, _)) = listener.accept() else {
                break;
            };
            if queue.len() < MAX_QUEUED {
                queue.push_back(stream);
            }
        }
        stats.peak_queued.fetch_max(queue.len(), Ordering::SeqCst);
        while stats.readers.load(Ordering::SeqCst) < MAX_READERS
            && let Some(stream) = queue.pop_front()
        {
            spawn_reader(stream, &sender, stats);
        }
        stats.queued.store(queue.len(), Ordering::SeqCst);
        if let Ok(callback) = receiver.recv_timeout(TICK)
            && let Some(outcome) = judge(callback, expect)
        {
            drop(listener);
            return outcome;
        }
    }
}

fn spawn_reader(stream: TcpStream, sender: &mpsc::Sender<Callback>, stats: &Arc<ListenerStats>) {
    let now = stats
        .readers
        .fetch_add(1, Ordering::SeqCst)
        .saturating_add(1);
    stats.peak_readers.fetch_max(now, Ordering::SeqCst);
    let sender = sender.clone();
    let count = Arc::clone(stats);
    let spawned = std::thread::Builder::new()
        .name("connector-sign-in".to_owned())
        .spawn(move || {
            if let Some(callback) = read_request(stream) {
                // The receiver is gone once the sign-in ended; the connection just closes.
                let _sent = sender.send(callback);
            }
            count.readers.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        stats.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The monotonic clock, for the per-connection deadline: the injected `Clock` is wall time and
/// may be a test's manual clock, and a wall clock stepped back must not stretch a deadline.
#[expect(
    clippy::disallowed_methods,
    reason = "the listener's per-connection deadline is real elapsed time on the monotonic clock"
)]
fn monotonic_now() -> Instant {
    Instant::now()
}

/// What is left of `deadline`, or `None` once it passed.
fn left(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(monotonic_now())
        .filter(|left| !left.is_zero())
}

/// Reads one request head, within [`HEAD_DEADLINE`] in all. Answers anything but
/// `GET /oauth/callback` itself (404, or 400 and 431 for a malformed or oversized head) and
/// returns `None`; a head not complete by the deadline is closed unanswered.
fn read_request(mut stream: TcpStream) -> Option<Callback> {
    let deadline = monotonic_now().checked_add(HEAD_DEADLINE)?;
    stream.set_nonblocking(false).ok()?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok()?;
    let mut head = Zeroizing::new(Vec::with_capacity(MAX_HEAD));
    let mut chunk = Zeroizing::new([0u8; 1024]);
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let room = MAX_HEAD.saturating_sub(head.len()).min(chunk.len());
        if room == 0 {
            respond(&mut stream, 431, "Request Header Fields Too Large");
            return None;
        }
        // The socket timeout is recomputed from what is left, so a client sending one byte at
        // a time still runs out at the deadline.
        stream.set_read_timeout(Some(left(deadline)?)).ok()?;
        let read = stream.read(&mut chunk[..room]).ok()?;
        if read == 0 {
            return None;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    let line_end = head.windows(2).position(|pair| pair == b"\r\n")?;
    let Ok(line) = std::str::from_utf8(&head[..line_end]) else {
        respond(&mut stream, 400, "Bad Request");
        return None;
    };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        respond(&mut stream, 400, "Bad Request");
        return None;
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != CALLBACK_PATH || !version.starts_with("HTTP/1.") {
        respond(&mut stream, 404, "Not Found");
        return None;
    }
    Some(Callback {
        stream,
        query: Zeroizing::new(query.to_owned()),
    })
}

/// The callback's checks, in order: `state` (wrong: 400 and keep waiting), `error`, `iss`, then
/// the code, handed back with the unanswered connection. A failed check answers the failure
/// page. `None` keeps waiting.
fn judge(
    mut callback: Callback,
    expect: &Expect<'_>,
) -> Option<Result<(Secret, Browser), SignInError>> {
    let pairs: Vec<(String, Zeroizing<String>)> =
        url::form_urlencoded::parse(callback.query.as_bytes())
            .map(|(name, value)| (name.into_owned(), Zeroizing::new(value.into_owned())))
            .collect();
    let one = |name: &str| {
        let mut values = pairs
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_str());
        match (values.next(), values.next()) {
            (Some(value), None) => Some(value),
            _ => None,
        }
    };
    let stream = &mut callback.stream;
    if one("state") != Some(expect.state.expose()) {
        respond(stream, 400, "Bad Request");
        return None;
    }
    if let Some((_, error)) = pairs.iter().find(|(key, _)| key == "error") {
        failed(stream);
        return Some(Err(if error.as_str() == "access_denied" {
            SignInError::Cancelled
        } else if is_error_code(error) {
            SignInError::Exchange(format!(
                "the authorization server answered {}",
                error.as_str()
            ))
        } else {
            SignInError::Exchange("the authorization server answered an error".to_owned())
        }));
    }
    if one("iss") != Some(expect.issuer) {
        failed(stream);
        return Some(Err(SignInError::IssuerMismatch));
    }
    let Some(code) = one("code").filter(|code| !code.is_empty()) else {
        failed(stream);
        return Some(Err(SignInError::Exchange(
            "the callback holds no code".to_owned(),
        )));
    };
    let code = Secret::from(code);
    Some(Ok((
        code,
        Browser {
            stream: callback.stream,
        },
    )))
}

/// The fixed failure page, with 400.
fn failed(stream: &mut TcpStream) {
    write_response(stream, 400, "Bad Request", Some(FAILURE_PAGE));
}

/// An answer with no body.
fn respond(stream: &mut TcpStream, status: u16, reason: &str) {
    write_response(stream, status, reason, None);
}

/// The policy both pages go out under: nothing loads, and the one style block runs only because
/// its SHA-256 is named here. The two pages share that block byte for byte, so one hash covers
/// both; editing it without updating this hash fails `the_page_policy_names_the_style_hash`.
pub(super) const PAGE_CSP: &str = "default-src 'none'; \
     style-src 'sha256-hck1SVO6a5kJkd0l4GCQ3jlobwVyjXVOzff2SnA7ioc='; img-src 'none'; \
     base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Writes one answer and closes the connection; a failed write is ignored (the browser left).
fn write_response(stream: &mut TcpStream, status: u16, reason: &str, page: Option<&str>) {
    let body = page.unwrap_or_default();
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    if page.is_some() {
        head.push_str("Content-Type: text/html; charset=utf-8\r\nContent-Security-Policy: ");
        head.push_str(PAGE_CSP);
        head.push_str("\r\n");
    }
    head.push_str(
        "Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: ",
    );
    head.push_str(&body.len().to_string());
    head.push_str("\r\n\r\n");
    let written = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .and_then(|()| stream.flush());
    if written.is_ok() {
        let _closed = stream.shutdown(std::net::Shutdown::Write);
    }
}
