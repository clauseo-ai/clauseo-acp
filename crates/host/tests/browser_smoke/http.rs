//! The test's HTTP: the page servers and the logging proxy that stands in for the host's.

use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) type Shared<T> = Arc<Mutex<T>>;

pub(crate) struct Request {
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Request {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

pub(crate) struct Response {
    pub(crate) status: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) body: Vec<u8>,
}

impl Response {
    pub(crate) fn ok(kind: &'static str, body: impl Into<Vec<u8>>) -> Self {
        let body = body.into();
        Self {
            status: "200 OK",
            kind,
            body,
        }
    }

    pub(crate) fn not_found() -> Self {
        Self {
            status: "404 Not Found",
            kind: "text/plain",
            body: b"not found".to_vec(),
        }
    }
}

/// The request line's method and target, and the headers.
type Head = (String, String, Vec<(String, String)>);

fn read_head(reader: &mut impl BufRead) -> io::Result<Head> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim_end().is_empty() {
            return Ok((method, target, headers));
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
}

/// Answers each connection to `listener` with one response from `handler`, then closes it.
pub(crate) fn serve<H>(listener: TcpListener, handler: H)
where
    H: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let handler = Arc::clone(&handler);
            thread::spawn(move || {
                if let Err(error) = answer(stream, &*handler) {
                    eprintln!("page server: {error}");
                }
            });
        }
    });
}

fn answer(mut stream: TcpStream, handler: &dyn Fn(&Request) -> Response) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let (_, target, headers) = read_head(&mut reader)?;
    let length = header(&headers, "content-length").map_or(0, |v| v.parse().unwrap_or(0));
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let response = handler(&Request {
        target,
        headers,
        body,
    });
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        response.status,
        response.kind,
        response.body.len()
    )?;
    stream.write_all(&response.body)?;
    stream.flush()
}

/// Starts the logging proxy: `smoke.test` goes to `127.0.0.1:<smoke_port>`, every other host
/// to itself. Each request's `host:port` is pushed to `log`.
pub(crate) fn proxy(listener: TcpListener, smoke_port: u16, log: Shared<Vec<String>>) {
    thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || {
                if let Err(error) = relay(client, smoke_port, &log) {
                    eprintln!("proxy: {error}");
                }
            });
        }
    });
}

fn relay(client: TcpStream, smoke_port: u16, log: &Mutex<Vec<String>>) -> io::Result<()> {
    let mut reader = BufReader::new(client.try_clone()?);
    let (method, target, headers) = read_head(&mut reader)?;
    // CONNECT carries `host:port`; anything else must be absolute-form `http://host/path`.
    let (authority, path) = if method == "CONNECT" {
        (target, None)
    } else {
        let rest = target
            .strip_prefix("http://")
            .ok_or_else(|| io::Error::other(format!("not absolute-form: {method} {target}")))?;
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        (authority.to_owned(), Some(format!("/{path}")))
    };
    let default_port = if path.is_some() { 80 } else { 443 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.parse::<u16>().is_ok() => {
            (host.to_owned(), port.parse().unwrap_or(default_port))
        }
        _ => (authority.clone(), default_port),
    };
    log.lock()
        .expect("proxy log")
        .push(format!("{host}:{port}"));
    let upstream = if host == "smoke.test" {
        TcpStream::connect(("127.0.0.1", smoke_port))?
    } else {
        TcpStream::connect((host.as_str(), port))?
    };
    let mut client = client;
    if let Some(path) = path {
        let mut head = format!("{method} {path} HTTP/1.1\r\n");
        for (key, value) in &headers {
            if !key.eq_ignore_ascii_case("proxy-connection")
                && !key.eq_ignore_ascii_case("connection")
            {
                write!(head, "{key}: {value}\r\n").expect("write to a String");
            }
        }
        head.push_str("Connection: close\r\nX-Smoke-Proxy: 1\r\n\r\n");
        (&upstream).write_all(head.as_bytes())?;
    } else {
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    }
    // Client to upstream on a thread (the reader drains its buffer first), upstream to
    // client here; when upstream ends, so does the client's connection.
    let mut to_upstream = upstream.try_clone()?;
    let up = thread::spawn(move || {
        io::copy(&mut reader, &mut to_upstream).unwrap_or_default();
        to_upstream.shutdown(Shutdown::Write).unwrap_or_default();
    });
    io::copy(&mut &upstream, &mut client).unwrap_or_default();
    client.shutdown(Shutdown::Both).unwrap_or_default();
    up.join().unwrap_or_default();
    Ok(())
}
