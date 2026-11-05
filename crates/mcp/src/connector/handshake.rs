//! The connection and the handshake. The first line the connector sends on a connection is
//! `{"clauseo_relay":1,"token":"<64 hex>","connector":"<name>"}`; the host answers
//! `{"clauseo_relay":1,"ok":true}`, or `ok:false` with an error code and closes. Any refusal, and
//! any failure to connect or to get an answer in time, is a reason the mode reports to the agent.

use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::line::{self, Line};

/// The reason when the relay cannot be reached or does not answer the handshake.
pub(crate) const CONNECT_FAILED: &str = "connect_failed";
/// How long the handshake may take in all, from the connect to the answer, and how long each
/// read or write may wait. The host answers at once.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest handshake answer. The host's answers are a few dozen bytes.
const ANSWER_MAX_BYTES: usize = 1024;

/// A connection past its handshake: the reader keeps the buffered bytes after the answer.
pub(crate) struct Link {
    pub(crate) reader: BufReader<TcpStream>,
    pub(crate) writer: TcpStream,
}

/// Connects to the relay at `addr` and completes the handshake for `connector`. The `Err` is the
/// reason for the agent: the host's error code when it refused, else [`CONNECT_FAILED`].
pub(crate) fn connect(addr: SocketAddr, token: &str, connector: &str) -> Result<Link, String> {
    let stream =
        TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT).map_err(|_io| CONNECT_FAILED)?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT)))
        .map_err(|_io| CONNECT_FAILED)?;
    let writer = stream.try_clone().map_err(|_io| CONNECT_FAILED)?;
    // One deadline for the whole handshake: the per-read timeouts above do not stop a relay that
    // sends a byte at a time. It lives until this function returns.
    let _deadline = Deadline::start(stream.try_clone().map_err(|_io| CONNECT_FAILED)?);
    let mut reader = BufReader::new(stream);
    send_hello(&writer, token, connector).map_err(|_io| CONNECT_FAILED)?;
    let Ok(Line::Text(answer)) = line::read(&mut reader, ANSWER_MAX_BYTES) else {
        return Err(CONNECT_FAILED.to_owned());
    };
    verdict(&answer)?;
    // The timeouts were for the handshake; the agent's session may be idle for long.
    reader
        .get_ref()
        .set_read_timeout(None)
        .and_then(|()| reader.get_ref().set_write_timeout(None))
        .map_err(|_no_reset| CONNECT_FAILED.to_owned())?;
    Ok(Link { reader, writer })
}

/// Shuts the connection down when the handshake has not finished within [`HANDSHAKE_TIMEOUT`]. The
/// handshake drops it when it returns: the watchdog then sees the disconnect and does nothing.
struct Deadline {
    _done: Sender<()>,
}

impl Deadline {
    fn start(socket: TcpStream) -> Self {
        let (done, expired) = mpsc::channel::<()>();
        thread::spawn(move || {
            if let Err(RecvTimeoutError::Timeout) = expired.recv_timeout(HANDSHAKE_TIMEOUT) {
                // Both halves: the handshake's read returns at once, as at an end.
                if socket.shutdown(Shutdown::Both).is_err() {
                    // Already gone: the handshake has ended some other way.
                }
            }
        });
        Self { _done: done }
    }
}

/// Writes the handshake line. The token is in it, so nothing reports this line's bytes.
fn send_hello(mut writer: &TcpStream, token: &str, connector: &str) -> io::Result<()> {
    let hello = format!(r#"{{"clauseo_relay":1,"token":"{token}","connector":"{connector}"}}"#);
    writer.write_all(hello.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// `Ok` for `clauseo_relay: 1` with `ok: true`. Otherwise the host's error code, if it is a plain
/// code, else the [`CONNECT_FAILED`] reason. The answer's own text is never returned.
fn verdict(answer: &[u8]) -> Result<(), String> {
    let Ok(value) = serde_json::from_slice::<Value>(answer) else {
        return Err(CONNECT_FAILED.to_owned());
    };
    // Exactly version 1 and `ok: true`; anything else, a missing version included, is a refusal.
    if value.get("clauseo_relay") == Some(&json!(1)) && value.get("ok") == Some(&Value::Bool(true))
    {
        return Ok(());
    }
    let code = value
        .get("error")
        .and_then(Value::as_str)
        .filter(|code| is_code(code))
        .unwrap_or(CONNECT_FAILED);
    Err(code.to_owned())
}

/// A host error code: 1 to 32 lowercase letters or underscores. Anything else is not echoed.
fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

#[cfg(test)]
mod tests;
