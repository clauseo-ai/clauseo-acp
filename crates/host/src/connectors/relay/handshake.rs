//! The handshake: the first line on a connection, `{"clauseo_relay":1,"token":"<64 hex>",
//! "connector":"<name>"}`, checked as strictly as `clauseo-mcp connector` checks the answer:
//! exactly these three keys, version 1, a token of 64 lowercase hex characters and a connector
//! name of 1 to 32 lowercase letters, digits or hyphens (not starting with one). A hello over
//! `MAX_HELLO_BYTES`, or one that does not end within [`HANDSHAKE_TIMEOUT`], closes the
//! connection unanswered.
//!
//! Until the token passes, every refusal is one answer, `bad_token`, then the close: a hello of
//! the wrong shape, another version, a wrong token and a revoked session's are told apart only in
//! the host's log ([`Refusal::Unauthenticated`]). Sandboxed matters can reach each other's
//! loopback ports, and the token is the boundary, so a client without it learns nothing of this
//! session's state (decision record 0050, "The hello"). Past the token, the answer is specific:
//! `unsupported_version`, `wrong_connector`, or `busy` or `closed` from `Session::promote`.
//! The hello is never logged, echoed or kept: it holds the token.

use std::io::{self, BufRead};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::future::{self, Either};
use serde::Deserialize;
use serde_json::Number;

use super::super::limits::HANDSHAKE_TIMEOUT;
use super::super::texts;
use super::{Session, hang_up, lock};

/// Why a handshake did not go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Closed with no answer; the reason is for the log.
    Silent(&'static str),
    /// Before the token passed: answered `bad_token` whatever the reason, then closed; the
    /// reason is for the log only.
    Unauthenticated(&'static str),
    /// Past the token: answered with this code, then closed.
    Answer(&'static str),
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// The stream ended (or failed) with no whole line.
    End,
    /// One line without its newline.
    Text(Vec<u8>),
    /// More than the cap before a newline; nothing more was read.
    TooLong,
}

/// Reads one line of at most `max` bytes. Past the cap it stops at once, so a peer that never
/// sends a newline holds no more than `max` bytes. A last line with no newline is not a line.
pub(crate) fn read_line(reader: &mut impl BufRead, max: usize) -> Line {
    let mut text = Vec::new();
    loop {
        let buffered = match reader.fill_buf() {
            Ok([]) | Err(_) => return Line::End,
            Ok(buffered) => buffered,
        };
        let (content, ended) = match buffered.iter().position(|&byte| byte == b'\n') {
            Some(index) => (buffered.get(..index).unwrap_or(buffered), true),
            None => (buffered, false),
        };
        if text.len().saturating_add(content.len()) > max {
            return Line::TooLong;
        }
        text.extend_from_slice(content);
        let consumed = content.len().saturating_add(usize::from(ended));
        reader.consume(consumed);
        if ended {
            return Line::Text(text);
        }
    }
}

/// Where a pending handshake's deadline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deadline {
    Waiting,
    Met,
    Expired,
}

/// Shuts the connection down when its handshake has not finished within
/// [`HANDSHAKE_TIMEOUT`] by the host's clock. The timer is armed when this is built.
pub(crate) struct Watchdog {
    state: Arc<Mutex<Deadline>>,
    _done: oneshot::Sender<()>,
}

impl Watchdog {
    pub(crate) fn arm(session: &Session, socket: TcpStream) -> io::Result<Watchdog> {
        let state = Arc::new(Mutex::new(Deadline::Waiting));
        let (done, finished) = oneshot::channel::<()>();
        let timer = session.hub.clock.after(HANDSHAKE_TIMEOUT);
        let watched = Arc::clone(&state);
        std::thread::Builder::new()
            .name("clauseo-relay-deadline".to_owned())
            .spawn(move || {
                if let Either::Left(((), _)) =
                    futures::executor::block_on(future::select(timer, finished))
                {
                    let mut deadline = lock(&watched);
                    if *deadline == Deadline::Waiting {
                        *deadline = Deadline::Expired;
                        hang_up(&socket);
                    }
                }
            })?;
        Ok(Watchdog { state, _done: done })
    }

    /// Marks the handshake done in time; `false` when the deadline passed first. Dropping the
    /// watchdog then ends its thread.
    pub(crate) fn meet(&self) -> bool {
        let mut deadline = lock(&self.state);
        if *deadline == Deadline::Waiting {
            *deadline = Deadline::Met;
        }
        *deadline == Deadline::Met
    }
}

/// Checks a hello line against `session`. The token is compared in constant time with this
/// listener's own, so a token of another listener is `bad_token`; a revoked session (its token
/// forgotten, `Session::close`) refuses every token the same way. Only a hello whose token
/// passes hears what else is wrong with it: another version, then another connector.
pub(crate) fn check(session: &Session, line: &[u8]) -> Result<(), Refusal> {
    let hello = parse(line).map_err(Refusal::Unauthenticated)?;
    match session.token_matches(hello.token) {
        Err(()) => return Err(Refusal::Unauthenticated(texts::CLOSED)),
        Ok(false) => return Err(Refusal::Unauthenticated(texts::BAD_TOKEN)),
        Ok(true) => {}
    }
    if hello.version.as_u64() != Some(1) {
        return Err(Refusal::Answer(texts::UNSUPPORTED_VERSION));
    }
    if hello.connector != session.hub.connector.known().id {
        return Err(Refusal::Answer(texts::WRONG_CONNECTOR));
    }
    Ok(())
}

/// A hello: exactly these keys, each once, the version a number. The strings borrow from the
/// line, so the token is not copied; one with an escape does not parse, and neither field may
/// hold one.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello<'a> {
    #[serde(rename = "clauseo_relay")]
    version: Number,
    token: &'a str,
    connector: &'a str,
}

/// The shape check; its `Err` is the reason for the log, `bad_hello`. Any number passes as the
/// version here: [`check`] answers another one only once the token has passed.
fn parse(line: &[u8]) -> Result<Hello<'_>, &'static str> {
    match serde_json::from_slice::<Hello<'_>>(line) {
        Ok(hello) if is_token(hello.token) && is_connector_name(hello.connector) => Ok(hello),
        _ => Err("bad_hello"),
    }
}

/// 64 lowercase hex characters.
fn is_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// 1 to 32 lowercase ASCII letters, digits or hyphens, not starting with a hyphen: the rule
/// `clauseo-mcp connector` holds its own name to.
fn is_connector_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests;
