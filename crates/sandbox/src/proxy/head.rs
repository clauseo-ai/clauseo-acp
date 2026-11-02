//! Request and response heads: reading one under the size cap, parsing it with `httparse`, and
//! the proxy's own short answers.

use std::future::Future;
use std::io;
use std::pin::pin;
use std::time::Duration;

use async_io::Timer;
use futures::future::{Either, select};
use futures::{AsyncRead, AsyncReadExt};

/// The most a head may take, request line and headers together.
pub(crate) const HEAD_CAP: usize = 8 * 1024;
/// The most header lines a head may have.
const MAX_HEADERS: usize = 100;

/// Why a head could not be read.
#[derive(Debug)]
pub(crate) enum HeadError {
    /// It passed [`HEAD_CAP`] or had too many headers: answered with 431.
    TooLarge,
    /// The peer closed before the head ended.
    Closed,
    /// The socket failed.
    Io(io::Error),
}

/// A head read off the wire: the head itself, through its blank line, and the bytes that came
/// after it in the same reads (the start of a body, or of tunnel traffic).
pub(crate) struct Head {
    pub(crate) bytes: Vec<u8>,
    pub(crate) rest: Vec<u8>,
}

/// Reads until the blank line that ends a head, at most [`HEAD_CAP`] bytes.
pub(crate) async fn read_head<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Head, HeadError> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        let room = HEAD_CAP.saturating_sub(buf.len()).min(chunk.len());
        if room == 0 {
            return Err(HeadError::TooLarge);
        }
        let read_into = chunk.get_mut(..room).unwrap_or_default();
        let n = reader.read(read_into).await.map_err(HeadError::Io)?;
        if n == 0 {
            return Err(HeadError::Closed);
        }
        let from = buf.len().saturating_sub(3);
        buf.extend_from_slice(read_into.get(..n).unwrap_or_default());
        let end = buf
            .get(from..)
            .and_then(|tail| tail.windows(4).position(|w| w == b"\r\n\r\n"))
            .map(|at| from + at + 4);
        if let Some(end) = end {
            let rest = buf.split_off(end);
            return Ok(Head { bytes: buf, rest });
        }
    }
}

/// `Some` with the future's output, or `None` when `limit` passes first.
pub(crate) async fn within<F: Future>(limit: Duration, future: F) -> Option<F::Output> {
    let future = pin!(future);
    match select(future, Timer::after(limit)).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}

/// A parsed request head. Header names keep their case; values are raw bytes.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, Vec<u8>)>,
}

impl Request {
    /// Every value of the header `name` (case-insensitive).
    pub(crate) fn values(&self, name: &str) -> Vec<&[u8]> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
            .collect()
    }
}

/// Why a request head was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// Too many headers: 431.
    TooLarge,
    /// Not HTTP/1.x: 400.
    Malformed,
}

/// Parses a complete request head.
pub(crate) fn parse_request(head: &[u8]) -> Result<Request, ParseError> {
    let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut slots);
    let status = request.parse(head).map_err(|error| {
        if matches!(error, httparse::Error::TooManyHeaders) {
            ParseError::TooLarge
        } else {
            ParseError::Malformed
        }
    })?;
    if status.is_partial() {
        return Err(ParseError::Malformed);
    }
    let (Some(method), Some(target)) = (request.method, request.path) else {
        return Err(ParseError::Malformed);
    };
    let headers = request
        .headers
        .iter()
        .map(|h| (h.name.to_owned(), h.value.to_vec()))
        .collect();
    Ok(Request {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
    })
}

/// The status code of a complete response head, or `None` if it is not one.
pub(crate) fn parse_status(head: &[u8]) -> Option<u16> {
    let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut response = httparse::Response::new(&mut slots);
    if response.parse(head).ok()?.is_partial() {
        return None;
    }
    response.code
}

/// The proxy's own answer: a status line, `extra` header lines (each ending in CRLF), no body.
pub(crate) fn answer(status: u16, extra: &str) -> Vec<u8> {
    let reason = match status {
        200 => "Connection Established",
        400 => "Bad Request",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    if status == 200 {
        return format!("HTTP/1.1 200 {reason}\r\n\r\n").into_bytes();
    }
    format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n{extra}\r\n")
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &[u8]) -> Result<Head, HeadError> {
        let mut reader = input;
        futures::executor::block_on(read_head(&mut reader))
    }

    #[test]
    fn a_head_and_what_follows_it() {
        let head = read(b"CONNECT a.test:443 HTTP/1.1\r\nHost: a.test\r\n\r\nhello").unwrap();
        assert_eq!(
            head.bytes,
            b"CONNECT a.test:443 HTTP/1.1\r\nHost: a.test\r\n\r\n"
        );
        assert_eq!(head.rest, b"hello");
        let request = parse_request(&head.bytes).unwrap();
        assert_eq!(request.method, "CONNECT");
        assert_eq!(request.target, "a.test:443");
        assert_eq!(request.values("host"), vec![b"a.test".as_slice()]);
    }

    #[test]
    fn heads_over_the_cap_or_cut_short_are_refused() {
        let mut big = b"GET http://a.test/ HTTP/1.1\r\nX: ".to_vec();
        big.resize(HEAD_CAP + 10, b'a');
        assert!(matches!(read(&big), Err(HeadError::TooLarge)));
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\n"),
            Err(HeadError::Closed)
        ));
        let mut many = b"GET / HTTP/1.1\r\n".to_vec();
        for i in 0..=MAX_HEADERS {
            many.extend_from_slice(format!("X-{i}: y\r\n").as_bytes());
        }
        many.extend_from_slice(b"\r\n");
        assert_eq!(parse_request(&many).unwrap_err(), ParseError::TooLarge);
        assert_eq!(
            parse_request(b"GET / HTTP/2.0\r\n\r\n").unwrap_err(),
            ParseError::Malformed
        );
        assert_eq!(
            parse_request(b"GET /\r\nX: y\r\n\r\n").unwrap_err(),
            ParseError::Malformed
        );
    }

    #[test]
    fn status_of_a_response() {
        assert_eq!(parse_status(b"HTTP/1.1 407 Auth\r\n\r\n"), Some(407));
        assert_eq!(parse_status(b"nonsense\r\n\r\n"), None);
    }
}
