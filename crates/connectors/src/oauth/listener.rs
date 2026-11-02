//! Step 7: the one-shot loopback listener. Each connection is read on its own thread, with a
//! 2-second read timeout and at most 8 KB of request head; only `GET /oauth/callback` reaches
//! the checks, anything else gets 404. A wrong or missing `state` gets 400 and the listener keeps
//! waiting, so no other page can end the sign-in. Then `error`, then `iss`, then the fixed page.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use zeroize::Zeroizing;

use super::{SUCCESS_PAGE, is_error_code};
use crate::cancel::CancelFlag;
use crate::config::Clock;
use crate::error::SignInError;
use crate::secret::Secret;

/// How often the wait looks at the cancel flag, the deadline and new connections.
const TICK: Duration = Duration::from_millis(20);
/// How long one connection may take to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// The most request head read from one connection.
const MAX_HEAD: usize = 8 * 1024;
/// Connections read at once; more are closed unanswered.
const MAX_READERS: usize = 8;
/// The callback path of the redirect URI.
const CALLBACK_PATH: &str = "/oauth/callback";

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

/// Waits for the callback, the deadline or the flag, and returns the code. The listener is
/// closed when this returns.
pub(super) fn wait_for_code(
    listener: TcpListener,
    expect: &Expect<'_>,
    cancel: &CancelFlag,
    clock: &dyn Clock,
    deadline: SystemTime,
) -> Result<Secret, SignInError> {
    let (sender, receiver) = mpsc::channel::<Callback>();
    let readers = Arc::new(AtomicUsize::new(0));
    loop {
        if cancel.is_cancelled() {
            return Err(SignInError::Cancelled);
        }
        if clock.now() >= deadline {
            return Err(SignInError::TimedOut);
        }
        // Every connection waiting to be accepted, each to a reader thread.
        while let Ok((stream, _)) = listener.accept() {
            spawn_reader(stream, &sender, &readers);
        }
        if let Ok(callback) = receiver.recv_timeout(TICK)
            && let Some(outcome) = judge(callback, expect)
        {
            drop(listener);
            return outcome;
        }
    }
}

fn spawn_reader(stream: TcpStream, sender: &mpsc::Sender<Callback>, readers: &Arc<AtomicUsize>) {
    if readers.fetch_add(1, Ordering::SeqCst) >= MAX_READERS {
        readers.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let sender = sender.clone();
    let count = Arc::clone(readers);
    let spawned = std::thread::Builder::new()
        .name("connector-sign-in".to_owned())
        .spawn(move || {
            if let Some(callback) = read_request(stream) {
                // The receiver is gone once the sign-in ended; the connection just closes.
                let _sent = sender.send(callback);
            }
            count.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        readers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Reads one request head. Answers anything but `GET /oauth/callback` itself (404, or 400 and
/// 431 for a malformed or oversized head) and returns `None`.
fn read_request(mut stream: TcpStream) -> Option<Callback> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(READ_TIMEOUT)).ok()?;
    let mut head = Zeroizing::new(Vec::with_capacity(MAX_HEAD));
    let mut chunk = Zeroizing::new([0u8; 1024]);
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let room = MAX_HEAD.saturating_sub(head.len()).min(chunk.len());
        if room == 0 {
            respond(&mut stream, 431, "Request Header Fields Too Large");
            return None;
        }
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
/// the code and the fixed page. `None` keeps waiting.
fn judge(mut callback: Callback, expect: &Expect<'_>) -> Option<Result<Secret, SignInError>> {
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
        respond(stream, 400, "Bad Request");
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
        respond(stream, 400, "Bad Request");
        return Some(Err(SignInError::IssuerMismatch));
    }
    let Some(code) = one("code").filter(|code| !code.is_empty()) else {
        respond(stream, 400, "Bad Request");
        return Some(Err(SignInError::Exchange(
            "the callback holds no code".to_owned(),
        )));
    };
    let code = Secret::from(code);
    write_response(stream, 200, "OK", Some(SUCCESS_PAGE));
    Some(Ok(code))
}

/// An answer with no body.
fn respond(stream: &mut TcpStream, status: u16, reason: &str) {
    write_response(stream, status, reason, None);
}

/// Writes one answer and closes the connection; a failed write is ignored (the browser left).
fn write_response(stream: &mut TcpStream, status: u16, reason: &str, page: Option<&str>) {
    let body = page.unwrap_or_default();
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    if page.is_some() {
        head.push_str(
            "Content-Type: text/html; charset=utf-8\r\n\
             Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n",
        );
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
