//! The connection and the handshake. The first line the connector sends on a connection is
//! `{"clauseo_relay":1,"token":"<64 hex>","connector":"<name>"}`; the host answers
//! `{"clauseo_relay":1,"ok":true}`, or `ok:false` with an error code and closes. Any refusal, and
//! any failure to connect or to get an answer in time, is a reason the mode reports to the agent.

use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::line::{self, Line};
use super::socket::{self, Polled, Stop};

/// The reason when the relay cannot be reached or does not answer the handshake.
pub(crate) const CONNECT_FAILED: &str = "connect_failed";
/// How long the connect may take, and then how long the hello and the answer may take in all.
/// The host answers at once.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The pauses between connect tries: five tries, about 1.5 s of pauses and 2 s in all on a
/// refused port, for a relay still starting as the agent starts this program. A relay that
/// answers, even with a refusal, is never tried again.
const CONNECT_PAUSES: [Duration; 4] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
];
/// The longest handshake answer. The host's answers are a few dozen bytes.
const ANSWER_MAX_BYTES: usize = 1024;

/// A connection past its handshake. The reader keeps the buffered bytes after the answer; its
/// stop is the handshake's deadline, which has fired by then, so the pump gives it its own before
/// it reads. The socket is the same one, for the stdin side's writes; it closes when both drop.
pub(crate) struct Link {
    pub(crate) reader: BufReader<Polled>,
    pub(crate) socket: Arc<TcpStream>,
}

/// Connects to the relay at `addr` and completes the handshake for `connector`. The `Err` is the
/// reason for the agent: the host's error code when it refused, else [`CONNECT_FAILED`].
pub(crate) fn connect(addr: SocketAddr, token: &str, connector: &str) -> Result<Link, String> {
    let stream = connect_with_retries(addr).map_err(|_io| CONNECT_FAILED)?;
    // One deadline for the hello and the answer together, so a relay that sends a byte at a time
    // is given up too. It is a stop that fires by itself; the writer and the reader check it
    // between their tries on the non-blocking socket, so nothing has to wake a blocked read. The
    // guard ends its timer when this function returns.
    let (_timer, deadline) = socket::stop_after(HANDSHAKE_TIMEOUT);
    stream.set_nonblocking(true).map_err(|_io| CONNECT_FAILED)?;
    let socket = Arc::new(stream);
    send_hello(&socket, token, connector, &deadline).map_err(|_io| CONNECT_FAILED)?;
    let reader = Polled::new(Arc::clone(&socket), deadline).map_err(|_io| CONNECT_FAILED)?;
    let mut reader = BufReader::new(reader);
    // On any failure the reader and the socket drop here, and the connection closes with them.
    let Ok(Line::Text(answer)) = line::read(&mut reader, ANSWER_MAX_BYTES) else {
        return Err(CONNECT_FAILED.to_owned());
    };
    verdict(&answer)?;
    Ok(Link { reader, socket })
}

/// Connects to `addr`, trying again after each of [`CONNECT_PAUSES`] when the connect fails.
fn connect_with_retries(addr: SocketAddr) -> io::Result<TcpStream> {
    let mut pauses = CONNECT_PAUSES.iter();
    loop {
        match TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => match pauses.next() {
                Some(pause) => wait(*pause),
                None => return Err(error),
            },
        }
    }
}

/// Waits for `after` (`clippy.toml` bans `thread::sleep`): the sender is held, so nothing
/// wakes the wait early.
fn wait(after: Duration) {
    let (_hold, wake) = std::sync::mpsc::channel::<()>();
    if wake.recv_timeout(after).is_err() {
        // The time is up, as the sender is still held.
    }
}

/// Writes the handshake line in one write. The token is in it, so nothing reports its bytes.
fn send_hello(socket: &TcpStream, token: &str, connector: &str, deadline: &Stop) -> io::Result<()> {
    let hello =
        format!("{{\"clauseo_relay\":1,\"token\":\"{token}\",\"connector\":\"{connector}\"}}\n");
    socket::write_all(socket, hello.as_bytes(), Some(deadline))
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
