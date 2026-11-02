//! A plain HTTP/1.1 client for tests, to loopback only: it drives the fake's `/authorize` and the
//! sign-in callback the way a browser would, one request per connection.

use std::io::{self, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use url::Url;

/// An HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    /// The status.
    pub status: u16,
    /// The headers, names lowercased, in order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl RawResponse {
    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(header, _)| *header == name)
            .map(|(_, value)| value.as_str())
    }

    /// The body as text, lossily.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Sends one request to `url`, which must be `http://127.0.0.1:<port>/...`; anything else is
/// refused before connecting, so a test never reaches the network. Redirects are not followed.
pub fn http(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<RawResponse> {
    let ("http", Some("127.0.0.1"), Some(port)) = (url.scheme(), url.host_str(), url.port()) else {
        return Err(io::Error::other(
            "the test client reaches http://127.0.0.1 only",
        ));
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut target = url.path().to_owned();
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (name, value) in headers {
        push_header(&mut head, name, value);
    }
    push_header(&mut head, "Content-Length", &body.len().to_string());
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse(&raw)
}

/// Appends `name: value\r\n`.
pub(super) fn push_header(head: &mut String, name: &str, value: &str) {
    head.push_str(name);
    head.push_str(": ");
    head.push_str(value);
    head.push_str("\r\n");
}

fn parse(raw: &[u8]) -> io::Result<RawResponse> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default());
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect::<Vec<_>>();
    let mut body = raw.get(split + 4..).unwrap_or_default().to_vec();
    if let Some(length) = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
    {
        body.truncate(length);
    }
    Ok(RawResponse {
        status,
        headers,
        body,
    })
}
