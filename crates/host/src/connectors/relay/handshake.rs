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
//!
//! Both listeners of a launch run this one copy ([`greet`]): the case-law relay and the
//! version-history listener (`connectors/versions_relay.rs`), each through its [`Door`].

use std::io::{self, BufRead, BufReader};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::future::{self, Either};
use serde::Deserialize;
use serde_json::Number;

use super::super::gate::TokenGate;
use super::super::limits::{HANDSHAKE_TIMEOUT, MAX_HELLO_BYTES};
use super::super::texts;
use super::conn::Writer;
use super::{hang_up, lock};
use crate::clock::Clock;

/// What a listener gives the handshake: its gate, its connector's name and its log.
pub(crate) trait Door {
    /// The listener's token and phase.
    fn gate(&self) -> &TokenGate;
    /// The name a hello must carry (`case-law`, `clauseo-versions`).
    fn connector(&self) -> &str;
    /// Logs a refused or dropped connection: the session and the reason, nothing it sent.
    fn log_refusal(&self, reason: &str);
}

/// The handshake on a new connection, whose deadline `watchdog` was armed at its accept: the
/// hello is read from `reader` and checked against `door`, then `admit` lets the connection
/// in (it answers `busy` or `closed`). `true` once the ok line went out on `writer`.
/// Otherwise the refusal is logged and the connection hung up, after one answer: `bad_token`
/// for every refusal before the token passed, the specific code after it, and none for a hello
/// that timed out, was too long or never came.
pub(crate) fn greet(
    door: &impl Door,
    reader: &mut BufReader<TcpStream>,
    watchdog: Watchdog,
    writer: &Writer,
    admit: impl FnOnce() -> Result<(), &'static str>,
) -> bool {
    let line = read_line(reader, MAX_HELLO_BYTES);
    let met = watchdog.meet();
    // Its thread ends now, and with the manual clock its timer is disarmed.
    drop(watchdog);
    if !met {
        door.log_refusal("handshake_timeout");
        return false;
    }
    let verdict = match line {
        Line::Text(text) => check(door.gate(), door.connector(), &text),
        Line::TooLong => Err(Refusal::Silent("hello_too_long")),
        Line::End => Err(Refusal::Silent("hung_up")),
    };
    let verdict = verdict.and_then(|()| admit().map_err(Refusal::Answer));
    // Before the token passed, every refusal answers as a wrong token's does; the log keeps
    // the reason.
    let (reason, code) = match verdict {
        Ok(()) => (None, None),
        Err(Refusal::Silent(reason)) => (Some(reason), None),
        Err(Refusal::Unauthenticated(reason)) => (Some(reason), Some(texts::BAD_TOKEN)),
        Err(Refusal::Answer(code)) => (Some(code), Some(code)),
    };
    if let Some(reason) = reason {
        door.log_refusal(reason);
        if let Some(code) = code
            && writer.line(&texts::hello_refused(code)).is_err()
        {
            // The peer is gone; it hears nothing either way.
        }
        writer.hang_up();
        return false;
    }
    if writer.line(&texts::hello_ok()).is_err() {
        writer.hang_up();
        return false;
    }
    true
}

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
    pub(crate) fn arm(clock: &dyn Clock, socket: TcpStream) -> io::Result<Watchdog> {
        let state = Arc::new(Mutex::new(Deadline::Waiting));
        let (done, finished) = oneshot::channel::<()>();
        let timer = clock.after(HANDSHAKE_TIMEOUT);
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

/// Checks a hello line against a listener's `gate` and its `connector` name. The token is
/// compared in constant time with this listener's own, so a token of another listener is
/// `bad_token`; a revoked listener (its token forgotten, `TokenGate::close`) refuses every token
/// the same way. Only a hello whose token passes hears what else is wrong with it: another
/// version, then another connector.
pub(crate) fn check(gate: &TokenGate, connector: &str, line: &[u8]) -> Result<(), Refusal> {
    let hello = parse(line).map_err(Refusal::Unauthenticated)?;
    match gate.token_matches(hello.token) {
        Err(()) => return Err(Refusal::Unauthenticated(texts::CLOSED)),
        Ok(false) => return Err(Refusal::Unauthenticated(texts::BAD_TOKEN)),
        Ok(true) => {}
    }
    if hello.version.as_u64() != Some(1) {
        return Err(Refusal::Answer(texts::UNSUPPORTED_VERSION));
    }
    if hello.connector != connector {
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
