//! The fake's HTTP/1.1 server on `std::net`: one thread per connection, one request per
//! connection (`Connection: close`), every request recorded before it is answered.

use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;

use super::client::push_header;
use super::{RecordedRequest, Shared, api, oauth};

/// The most a request's head may hold.
const MAX_HEAD: usize = 64 * 1024;
/// The most a request's body may hold.
const MAX_BODY: usize = 1024 * 1024;

/// An answer.
#[derive(Debug, Clone)]
pub(super) struct Response {
    pub(super) status: u16,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

impl Response {
    /// `status` with a JSON body.
    pub(super) fn json(status: u16, value: &Value) -> Response {
        Response {
            status,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: serde_json::to_vec(value).unwrap_or_default(),
        }
    }

    /// `status` with a plain-text body.
    pub(super) fn text(status: u16, text: &str) -> Response {
        Response {
            status,
            headers: vec![(
                "Content-Type".to_owned(),
                "text/plain; charset=utf-8".to_owned(),
            )],
            body: text.as_bytes().to_vec(),
        }
    }

    /// `status` with no body.
    pub(super) fn empty(status: u16) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Adds a header.
    pub(super) fn with(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// Accepts connections until `shared.stop` is set, each on its own thread.
pub(super) fn spawn(listener: TcpListener, shared: Arc<Shared>) -> io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("fake-connector".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                if shared.stop.is_cancelled() {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let shared = Arc::clone(&shared);
                let spawned = std::thread::Builder::new()
                    .name("fake-connection".to_owned())
                    .spawn(move || serve(&stream, &shared));
                if let Err(err) = spawned {
                    tracing::warn!(error = %err, "fake connector: no thread for a connection");
                }
            }
        })
}

/// Reads one request, records it, answers it.
fn serve(stream: &TcpStream, shared: &Shared) {
    if let Err(err) = stream.set_read_timeout(Some(Duration::from_secs(5))) {
        tracing::warn!(error = %err, "fake connector: no read timeout");
    }
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let request = match read_request(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(_) => {
            write_response(stream, &Response::text(400, "bad request"));
            return;
        }
    };
    shared
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .requests
        .push(request.clone());
    let response = answer(shared, &request);
    write_response(stream, &response);
}

/// The request's head and body; `None` when the peer closed before sending anything.
fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<RecordedRequest>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    if method.is_empty() || !target.starts_with('/') {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut headers = Vec::new();
    let mut head = line.len();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        head = head.saturating_add(read);
        if read == 0 || head > MAX_HEAD {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(io::ErrorKind::InvalidData)?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map_or(Ok(0), |(_, value)| value.parse::<usize>())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if length > MAX_BODY {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target, String::new()),
    };
    Ok(Some(RecordedRequest {
        method,
        path,
        query,
        headers,
        body,
    }))
}

fn write_response(mut stream: &TcpStream, response: &Response) {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    for (name, value) in &response.headers {
        push_header(&mut head, name, value);
    }
    push_header(
        &mut head,
        "Content-Length",
        &response.body.len().to_string(),
    );
    head.push_str("Connection: close\r\n\r\n");
    let written = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&response.body))
        .and_then(|()| stream.flush());
    if let Err(err) = written {
        tracing::debug!(error = %err, "fake connector: the client went away");
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// The injected failure or delay for this path, then the endpoint's answer.
fn answer(shared: &Shared, request: &RecordedRequest) -> Response {
    let (fail, delay) = {
        let mut state = shared.state.lock().unwrap_or_else(PoisonError::into_inner);
        let fail = state
            .fail_next
            .get_mut(&request.path)
            .and_then(std::collections::VecDeque::pop_front);
        let delay = state
            .delay_next
            .get_mut(&request.path)
            .and_then(std::collections::VecDeque::pop_front);
        (fail, delay)
    };
    if let Some(delay) = delay
        && shared.stop.wait_timeout(delay)
    {
        return Response::text(503, "the fake connector is stopping");
    }
    if let Some(status) = fail {
        return Response::json(
            status,
            &serde_json::json!({"error": {"code": "injected", "status": status}}),
        );
    }
    route(shared, request)
}

fn route(shared: &Shared, request: &RecordedRequest) -> Response {
    let method = request.method.as_str();
    let path = request.path.as_str();
    match (method, path) {
        ("GET", "/.well-known/oauth-protected-resource/mcp/case-law") => {
            Response::json(200, &shared.lock().prm)
        }
        ("GET", "/.well-known/oauth-authorization-server") => {
            Response::json(200, &shared.lock().as_metadata)
        }
        ("GET", "/oauth/clients/clauseo-desktop.json") => {
            Response::json(200, &shared.lock().client_metadata)
        }
        ("GET", "/authorize") => oauth::authorize(shared, request),
        ("POST", "/token") => oauth::token(shared, request),
        ("POST", "/revoke") => oauth::revoke(shared, request),
        ("POST", "/mcp/case-law") => api::mcp(shared, request),
        ("GET", "/v1/case-law/account") => api::account(shared, request),
        ("POST", "/v1/case-law/recharges") => api::create_recharge(shared, request),
        ("GET", "/pricing.json") => api::pricing(shared),
        ("GET", _) => {
            if let Some(id) = path.strip_prefix("/v1/case-law/recharges/") {
                api::get_recharge(shared, request, id)
            } else if path.starts_with("/pay/") {
                Response {
                    status: 200,
                    headers: vec![(
                        "Content-Type".to_owned(),
                        "text/html; charset=utf-8".to_owned(),
                    )],
                    body: b"<p>The fake connector's pay page.</p>".to_vec(),
                }
            } else {
                not_found()
            }
        }
        _ => not_found(),
    }
}

fn not_found() -> Response {
    Response::json(404, &serde_json::json!({"error": {"code": "not_found"}}))
}
