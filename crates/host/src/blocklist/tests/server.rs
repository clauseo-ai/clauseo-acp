//! A fake HTTP server for the block list's tests: 127.0.0.1, canned answers by path, one
//! connection at a time, each answer closing its connection. As a proxy ([`Server::proxy`]) it
//! takes a `CONNECT`, answers 200 and then serves the tunnelled request from its own routes,
//! which are the target server's.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// One canned answer.
#[derive(Clone)]
pub(super) enum Reply {
    /// 200 with this body.
    Body(Vec<u8>),
    /// A 302 to this `Location`.
    Redirect(String),
    /// This status, no body.
    Status(u16),
}

/// The routes: path to answer (a path with none answers 404).
type Routes = Arc<Mutex<HashMap<String, Reply>>>;

/// The server: its port, its answers and every request line's target it saw.
pub(super) struct Server {
    port: u16,
    routes: Routes,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// Starts serving on a free port of 127.0.0.1, on a thread that ends with the test's process.
    pub(super) fn start() -> Server {
        Self::serving(Arc::new(Mutex::new(HashMap::new())))
    }

    /// A proxy in front of `target`: it sees the `CONNECT`, then serves `target`'s routes.
    pub(super) fn proxy(target: &Server) -> Server {
        Self::serving(Arc::clone(&target.routes))
    }

    fn serving(routes: Routes) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let (served, seen) = (Arc::clone(&routes), Arc::clone(&hits));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(&stream, &served, &seen);
            }
        });
        Server { port, routes, hits }
    }

    /// The URL of `path` on this server.
    pub(super) fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// The port.
    pub(super) fn port(&self) -> u16 {
        self.port
    }

    /// Answers `path` with `reply` from now on.
    pub(super) fn set(&self, path: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(path.to_owned(), reply);
    }

    /// Each request's method and target so far (`GET /list.gz`, `CONNECT 127.0.0.1:1234`).
    pub(super) fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }
}

/// Reads requests and writes their canned answers: a `CONNECT` gets 200 and the next request
/// on the same connection is served; a client gone early is fine.
fn answer(stream: &TcpStream, routes: &Mutex<HashMap<String, Reply>>, hits: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream);
    let mut writer = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let mut words = line.split(' ');
        let method = words.next().unwrap_or_default().to_owned();
        let path = words.next().unwrap_or_default().to_owned();
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) if header == "\r\n" => break,
                Ok(_) => {}
            }
        }
        hits.lock().unwrap().push(format!("{method} {path}"));
        if method == "CONNECT" {
            if writer.write_all(b"HTTP/1.1 200 OK\r\n\r\n").is_err() {
                return;
            }
            continue;
        }
        let reply = routes.lock().unwrap().get(&path).cloned();
        let (status, location, body) = match reply {
            Some(Reply::Body(body)) => (200, None, body),
            Some(Reply::Redirect(to)) => (302, Some(to), Vec::new()),
            Some(Reply::Status(status)) => (status, None, Vec::new()),
            None => (404, None, Vec::new()),
        };
        let mut head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if let Some(location) = location {
            head = format!("{head}Location: {location}\r\n");
        }
        head.push_str("\r\n");
        drop(
            writer
                .write_all(head.as_bytes())
                .and_then(|()| writer.write_all(&body)),
        );
        return;
    }
}
