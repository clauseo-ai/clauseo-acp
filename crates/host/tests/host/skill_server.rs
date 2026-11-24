//! A fake HTTP server for the skill downloads' tests (`add_skills.rs`): 127.0.0.1, canned
//! answers by request target (path and query), one connection at a time, each answer closing
//! its connection; every target asked, and each request's header lines, are kept. The host
//! reaches it through `HostConfig::with_skill_server`, so no test touches the network.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// The routes: target to (status, body); a target with none answers 404.
type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

/// The server.
pub(crate) struct SkillServer {
    port: u16,
    routes: Routes,
    hits: Arc<Mutex<Vec<String>>>,
    heads: Arc<Mutex<Vec<Vec<String>>>>,
}

impl SkillServer {
    /// Serves on a free port, on a thread that ends with the test's process.
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let routes: Routes = Arc::default();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let (served, seen, kept) = (Arc::clone(&routes), Arc::clone(&hits), Arc::clone(&heads));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                answer(&stream, &served, (&seen, &kept));
            }
        });
        Self {
            port,
            routes,
            hits,
            heads,
        }
    }

    /// Its origin, for `with_skill_server`.
    pub(crate) fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Answers `target` with 200 and `body` from now on.
    pub(crate) fn body(&self, target: &str, body: Vec<u8>) {
        self.routes
            .lock()
            .unwrap()
            .insert(target.to_owned(), (200, body));
    }

    /// Every target asked so far.
    pub(crate) fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }

    /// Each request's header lines so far, as sent, in order.
    pub(crate) fn heads(&self) -> Vec<Vec<String>> {
        self.heads.lock().unwrap().clone()
    }
}

/// Reads one request and writes its canned answer.
fn answer(
    stream: &TcpStream,
    routes: &Mutex<HashMap<String, (u16, Vec<u8>)>>,
    (hits, heads): (&Mutex<Vec<String>>, &Mutex<Vec<Vec<String>>>),
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let target = line.split(' ').nth(1).unwrap_or_default().to_owned();
    let mut lines = Vec::new();
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => lines.push(header.trim_end().to_owned()),
        }
    }
    hits.lock().unwrap().push(target.clone());
    heads.lock().unwrap().push(lines);
    let (status, body) = routes
        .lock()
        .unwrap()
        .get(&target)
        .cloned()
        .unwrap_or((404, Vec::new()));
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut writer = stream;
    drop(
        writer
            .write_all(head.as_bytes())
            .and_then(|()| writer.write_all(&body)),
    );
}

/// A zip of `entries` (name, bytes), deflated.
pub(crate) fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
