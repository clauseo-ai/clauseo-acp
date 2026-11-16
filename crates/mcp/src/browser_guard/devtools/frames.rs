//! Websocket framing (RFC 6455 section 5) for the relay's one client: masked frames in, unmasked
//! frames out, no extensions. Text and binary messages may come fragmented; control frames
//! (ping, pong, close) may come between fragments. A message over [`MAX_MESSAGE`] and any frame
//! the RFC forbids (an unmasked client frame, a reserved bit or opcode, a fragmented or long
//! control frame, a continuation with nothing to continue) end the connection.

use std::io::{self, Read, Write};

/// The largest message either way: Chromium's own `DevTools` receive buffer
/// (`devtools_pipe_handler.cc`, `kReceiveBufferSizeForDevTools`).
pub(crate) const MAX_MESSAGE: usize = 100 * 1024 * 1024;

const CONTINUATION: u8 = 0x0;
const TEXT: u8 = 0x1;
const BINARY: u8 = 0x2;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;

/// One message from the client.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Message {
    /// A whole text message, or a binary one that is valid UTF-8 (CDP is JSON either way).
    Text(String),
    /// A ping, to be answered with a pong carrying the same bytes.
    Ping(Vec<u8>),
    /// A pong: nothing to do.
    Pong,
    /// The client's close, or the end of its stream.
    Close,
}

/// Reads whole messages from a client stream.
pub(crate) struct Reader<R> {
    input: R,
    max: usize,
    /// The fragments of a text or binary message so far.
    partial: Option<Vec<u8>>,
}

impl<R: Read> Reader<R> {
    pub(crate) fn new(input: R) -> Self {
        Self {
            input,
            max: MAX_MESSAGE,
            partial: None,
        }
    }

    /// A reader whose messages may be at most `max` bytes, for tests.
    #[cfg(test)]
    pub(crate) fn with_max(input: R, max: usize) -> Self {
        Self {
            input,
            max,
            partial: None,
        }
    }

    /// The next message. A clean end of the stream between frames reads as [`Message::Close`];
    /// a frame the RFC forbids, or a message over the limit, is an error.
    pub(crate) fn next(&mut self) -> io::Result<Message> {
        loop {
            let mut head = [0_u8; 2];
            if !read_or_end(&mut self.input, &mut head)? {
                return Ok(Message::Close);
            }
            let [first, second] = head;
            let fin = first & 0x80 != 0;
            let opcode = first & 0x0F;
            if first & 0x70 != 0 {
                return Err(bad("a reserved bit is set"));
            }
            if second & 0x80 == 0 {
                return Err(bad("a client frame is not masked"));
            }
            let len = self.length(second & 0x7F)?;
            let control = opcode & 0x08 != 0;
            if control && (!fin || len > 125) {
                return Err(bad("a control frame is fragmented or too long"));
            }
            let mut mask = [0_u8; 4];
            self.input.read_exact(&mut mask)?;
            let so_far = self.partial.as_ref().map_or(0, Vec::len);
            if !control && so_far.saturating_add(len) > self.max {
                return Err(bad("a message is over the limit"));
            }
            let mut payload = vec![0_u8; len];
            self.input.read_exact(&mut payload)?;
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask.get(i % 4).copied().unwrap_or(0);
            }
            match opcode {
                PING => return Ok(Message::Ping(payload)),
                PONG => return Ok(Message::Pong),
                CLOSE => return Ok(Message::Close),
                TEXT | BINARY if self.partial.is_some() => {
                    return Err(bad("a new message began inside a fragmented one"));
                }
                TEXT | BINARY => self.partial = Some(payload),
                CONTINUATION => match self.partial.as_mut() {
                    Some(partial) => partial.extend_from_slice(&payload),
                    None => return Err(bad("a continuation continues nothing")),
                },
                _ => return Err(bad("a reserved opcode")),
            }
            if fin && let Some(whole) = self.partial.take() {
                return String::from_utf8(whole)
                    .map(Message::Text)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
        }
    }

    /// The payload length a frame's second byte starts.
    fn length(&mut self, short: u8) -> io::Result<usize> {
        let long = match short {
            126 => {
                let mut bytes = [0_u8; 2];
                self.input.read_exact(&mut bytes)?;
                u64::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0_u8; 8];
                self.input.read_exact(&mut bytes)?;
                u64::from_be_bytes(bytes)
            }
            short => u64::from(short),
        };
        usize::try_from(long)
            .ok()
            .filter(|len| *len <= self.max)
            .ok_or_else(|| bad("a frame is over the limit"))
    }
}

/// Fills `buf`, or returns `false` when the stream ends before its first byte.
fn read_or_end(input: &mut impl Read, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(buf.get_mut(filled..).unwrap_or_default()) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("websocket: {what}"))
}

/// Writes one unmasked text frame holding `payload`.
pub(crate) fn write_text(output: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    write_frame(output, TEXT, payload)
}

/// Writes a pong answering a ping that carried `payload`.
pub(crate) fn write_pong(output: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    write_frame(output, PONG, payload)
}

/// Writes a close frame with no status.
pub(crate) fn write_close(output: &mut impl Write) -> io::Result<()> {
    write_frame(output, CLOSE, &[])
}

fn write_frame(output: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | opcode);
    let len = payload.len();
    if let Ok(short @ 0..=125) = u8::try_from(len) {
        head.push(short);
    } else if let Ok(medium) = u16::try_from(len) {
        head.push(126);
        head.extend_from_slice(&medium.to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&u64::try_from(len).unwrap_or(u64::MAX).to_be_bytes());
    }
    output.write_all(&head)?;
    output.write_all(payload)?;
    output.flush()
}
