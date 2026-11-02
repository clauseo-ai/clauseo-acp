//! Moving bytes: the raw copy of a tunnel or a response, and the copy of exactly one request body
//! (by `Content-Length`, or by chunked framing read as it passes, bytes unchanged).

use std::cell::Cell;
use std::io;
use std::net::{Shutdown, TcpStream};

use async_io::Async;
use futures::{AsyncReadExt, AsyncWriteExt};

use crate::proxy::head::HEAD_CAP;

/// One socket of a relay.
pub(crate) type Stream = Async<TcpStream>;

/// The size of each copy's buffer.
const BUF: usize = 16 * 1024;

/// Copies `from` to `to` until `from` ends, counting bytes, then ends `to`'s write half so the
/// other side sees the end too.
pub(crate) async fn pump(from: &Stream, to: &Stream, count: &Cell<u64>) -> io::Result<()> {
    let mut buf = vec![0u8; BUF];
    loop {
        let n = (&*from).read(&mut buf).await?;
        if n == 0 {
            break;
        }
        (&*to).write_all(buf.get(..n).unwrap_or_default()).await?;
        count.set(count.get().saturating_add(n as u64));
    }
    end_writes(to);
    Ok(())
}

/// Ends the write half of `stream`. A socket the peer already closed is fine.
pub(crate) fn end_writes(stream: &Stream) {
    if let Err(error) = stream.get_ref().shutdown(Shutdown::Write) {
        tracing::trace!(%error, "egress: write half already closed");
    }
}

/// How a request's body ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framing {
    /// No body.
    Empty,
    /// `Content-Length` bytes.
    Length(u64),
    /// `Transfer-Encoding: chunked`.
    Chunked,
}

/// Writes the body's first bytes (`rest`, read with the head) and then the rest of it from
/// `client` to `origin`, unchanged, and stops at its end: bytes after it are never sent, so a
/// second request on the same connection cannot reach the origin.
pub(crate) async fn copy_body(
    framing: Framing,
    rest: &[u8],
    client: &Stream,
    origin: &Stream,
    count: &Cell<u64>,
) -> io::Result<()> {
    let mut framer = match framing {
        Framing::Empty => return Ok(()),
        Framing::Length(n) => Framer::Length(n),
        Framing::Chunked => Framer::Chunked(Chunked::start()),
    };
    let mut buf = vec![0u8; BUF];
    let mut input: &[u8] = rest;
    loop {
        let (taken, done) = framer.take(input)?;
        let body = input.get(..taken).unwrap_or_default();
        (&*origin).write_all(body).await?;
        count.set(count.get().saturating_add(taken as u64));
        if done {
            return Ok(());
        }
        let n = (&*client).read(&mut buf).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        input = buf.get(..n).unwrap_or_default();
    }
}

#[derive(Debug)]
enum Framer {
    Length(u64),
    Chunked(Chunked),
}

impl Framer {
    /// How many of `bytes` belong to the body, and whether the body ends with them.
    fn take(&mut self, bytes: &[u8]) -> io::Result<(usize, bool)> {
        match self {
            Framer::Length(left) => {
                let n = usize::try_from(*left)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                *left -= n as u64;
                Ok((n, *left == 0))
            }
            Framer::Chunked(state) => state.take(bytes),
        }
    }
}

/// Where a chunked body's reader stands. Strict: CRLF line ends only, at most 16 hex digits,
/// trailers at most [`HEAD_CAP`] bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunked {
    Size { digits: u8, value: u64, ext: bool },
    SizeLf { value: u64 },
    Data(u64),
    DataCr,
    DataLf,
    LineStart { trailer: usize },
    Line { trailer: usize },
    LineLf { trailer: usize },
    EndLf,
    Done,
}

fn bad_chunk() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "malformed chunked body")
}

impl Chunked {
    fn start() -> Chunked {
        Chunked::Size {
            digits: 0,
            value: 0,
            ext: false,
        }
    }

    fn take(&mut self, bytes: &[u8]) -> io::Result<(usize, bool)> {
        let mut rest = bytes;
        while *self != Chunked::Done {
            if let Chunked::Data(left) = *self {
                if rest.is_empty() {
                    break;
                }
                let n = usize::try_from(left).unwrap_or(usize::MAX).min(rest.len());
                rest = rest.get(n..).unwrap_or_default();
                let left = left - n as u64;
                *self = if left == 0 {
                    Chunked::DataCr
                } else {
                    Chunked::Data(left)
                };
                continue;
            }
            let Some((&byte, tail)) = rest.split_first() else {
                break;
            };
            rest = tail;
            *self = self.step(byte)?;
        }
        Ok((bytes.len() - rest.len(), *self == Chunked::Done))
    }

    fn step(self, b: u8) -> io::Result<Chunked> {
        let next = match self {
            Chunked::Size { digits, value, ext } => {
                if b == b'\r' && digits > 0 {
                    Chunked::SizeLf { value }
                } else if ext && b != b'\n' && b != b'\r' {
                    self
                } else if let Some(d) = (b as char).to_digit(16).filter(|_| !ext) {
                    if digits >= 16 {
                        return Err(bad_chunk());
                    }
                    Chunked::Size {
                        digits: digits + 1,
                        value: value << 4 | u64::from(d),
                        ext,
                    }
                } else if matches!(b, b';' | b' ' | b'\t') && digits > 0 {
                    Chunked::Size {
                        digits,
                        value,
                        ext: true,
                    }
                } else {
                    return Err(bad_chunk());
                }
            }
            Chunked::SizeLf { value } if b == b'\n' => {
                if value == 0 {
                    Chunked::LineStart { trailer: 0 }
                } else {
                    Chunked::Data(value)
                }
            }
            Chunked::DataCr if b == b'\r' => Chunked::DataLf,
            Chunked::DataLf if b == b'\n' => Chunked::start(),
            Chunked::LineStart { .. } if b == b'\r' => Chunked::EndLf,
            Chunked::LineStart { trailer } | Chunked::Line { trailer } if b != b'\n' => {
                if b == b'\r' {
                    Chunked::LineLf { trailer }
                } else if trailer >= HEAD_CAP {
                    return Err(bad_chunk());
                } else {
                    Chunked::Line {
                        trailer: trailer + 1,
                    }
                }
            }
            Chunked::LineLf { trailer } if b == b'\n' => Chunked::LineStart { trailer },
            Chunked::EndLf if b == b'\n' => Chunked::Done,
            Chunked::Done => Chunked::Done,
            Chunked::SizeLf { .. }
            | Chunked::Data(_)
            | Chunked::DataCr
            | Chunked::DataLf
            | Chunked::LineStart { .. }
            | Chunked::Line { .. }
            | Chunked::LineLf { .. }
            | Chunked::EndLf => return Err(bad_chunk()),
        };
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunked(input: &[u8]) -> io::Result<(usize, bool)> {
        Chunked::start().take(input)
    }

    #[test]
    fn a_chunked_body_ends_where_it_says() {
        let body = b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\n";
        let mut input = body.to_vec();
        input.extend_from_slice(b"GET http://other.test/ HTTP/1.1\r\n\r\n");
        assert_eq!(chunked(&input).unwrap(), (body.len(), true));
        assert_eq!(chunked(b"0\r\n\r\n").unwrap(), (5, true));
    }

    #[test]
    fn a_chunked_body_split_anywhere_reads_the_same() {
        let body = b"a\r\n0123456789\r\n0\r\n\r\nNEXT";
        for cut in 0..body.len() {
            let (first, second) = body.split_at(cut);
            let mut state = Chunked::start();
            let (a, done_a) = state.take(first).unwrap();
            if done_a {
                assert_eq!(a, body.len() - 4, "cut {cut}");
                continue;
            }
            assert_eq!(a, first.len(), "cut {cut}");
            let (b, done_b) = state.take(second).unwrap();
            assert!(done_b, "cut {cut}");
            assert_eq!(a + b, body.len() - 4, "cut {cut}");
        }
    }

    #[test]
    fn malformed_chunks_are_refused() {
        for bad in [
            &b"\r\n"[..],
            b"g\r\n",
            b"4\nWiki\r\n",
            b"4\r\nWikiX\r\n",
            b"11111111111111111\r\n",
            b"0\r\n\n",
            b";x\r\n",
        ] {
            assert!(chunked(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn a_length_body_stops_at_its_length() {
        let mut framer = Framer::Length(5);
        assert_eq!(framer.take(b"abc").unwrap(), (3, false));
        assert_eq!(framer.take(b"defgh").unwrap(), (2, true));
    }
}
