//! The upload's endpoint rules and what each POST carries: the request as built, and the bytes
//! sent to a server on 127.0.0.1. No request leaves the machine.

use std::collections::BTreeSet;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;

use super::*;

#[test]
fn only_a_plain_https_origin_is_an_endpoint() {
    assert_eq!(
        endpoint("https://t.clauseo.chat"),
        Some("https://t.clauseo.chat".to_owned())
    );
    assert_eq!(
        endpoint("https://127.0.0.1:8787/"),
        Some("https://127.0.0.1:8787".to_owned())
    );
    for refused in [
        "",
        "http://t.clauseo.chat",
        "ftp://t.clauseo.chat",
        "https://user@t.clauseo.chat",
        "https://user:pass@t.clauseo.chat",
        "https://t.clauseo.chat/v1",
        "https://t.clauseo.chat/?a=1",
        "https://t.clauseo.chat/#a",
        "t.clauseo.chat",
        "file:///etc/passwd",
    ] {
        assert_eq!(endpoint(refused), None, "{refused}");
    }
}

#[test]
fn release_builds_have_an_empty_endpoint_and_ignore_the_variable() {
    assert_eq!(RELEASE_ENDPOINT, "");
    assert_eq!(endpoint(RELEASE_ENDPOINT), None);
}

#[test]
fn the_built_request_names_three_headers_and_the_gzip_body() {
    let upload = HttpsUpload::to(Some("https://t.clauseo.chat"));
    let request = upload
        .request(UploadPath::Counts, vec![0x1f, 0x8b, 1, 2])
        .unwrap();
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(request.url().as_str(), "https://t.clauseo.chat/v1/counts");
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        headers,
        [
            ("user-agent".to_owned(), "Clauseo".to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
            ("content-encoding".to_owned(), "gzip".to_owned()),
        ]
    );
    assert_eq!(
        request.body().and_then(reqwest::Body::as_bytes),
        Some(&[0x1f, 0x8b, 1, 2][..])
    );
    let tally = upload.request(UploadPath::Tally, Vec::new()).unwrap();
    assert_eq!(tally.url().as_str(), "https://t.clauseo.chat/v1/tally");
}

#[test]
fn with_no_endpoint_a_post_fails_without_the_network() {
    let upload = HttpsUpload::to(None);
    assert!(upload.request(UploadPath::Counts, Vec::new()).is_none());
    let answer = futures::executor::block_on(upload.post(UploadPath::Counts, vec![1]));
    assert_eq!(answer, Err(UploadError));
    let plain_http = HttpsUpload::to(Some("http://127.0.0.1:1"));
    let answer = futures::executor::block_on(plain_http.post(UploadPath::Counts, vec![1]));
    assert_eq!(answer, Err(UploadError));
}

/// The request a server on 127.0.0.1 reads: its request line and headers, lower-cased names.
fn serve_one(listener: &TcpListener) -> (String, BTreeSet<(String, String)>, Vec<u8>) {
    let (stream, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut headers = BTreeSet::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_owned());
        if name == "content-length" {
            length = value.parse().unwrap();
        }
        assert!(headers.insert((name, value)), "a header twice: {line}");
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    reader
        .get_mut()
        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        .unwrap();
    (request_line.trim_end().to_owned(), headers, body)
}

/// What goes on the wire, read by a plain-HTTP server on 127.0.0.1: the app's client settings
/// (`builder`), with HTTPS only relaxed for the test alone and no proxy, so the server reads the
/// request itself. Exactly these headers and no other: a reqwest that adds one fails here.
#[test]
fn the_bytes_sent_hold_exactly_the_disclosed_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || serve_one(&listener));
    let upload = HttpsUpload {
        client: Some(builder().https_only(false).no_proxy().build().unwrap()),
        endpoint: Some(format!("http://127.0.0.1:{port}")),
    };
    let body = vec![0x1f, 0x8b, 8, 0];
    let status = tauri::async_runtime::block_on(upload.post(UploadPath::Counts, body.clone()));
    assert_eq!(status, Ok(200));
    let (request_line, headers, sent) = server.join().unwrap();
    assert_eq!(request_line, "POST /v1/counts HTTP/1.1");
    let expected: BTreeSet<(String, String)> = [
        ("user-agent", "Clauseo".to_owned()),
        ("content-type", "application/json".to_owned()),
        ("content-encoding", "gzip".to_owned()),
        ("accept", "*/*".to_owned()),
        ("host", format!("127.0.0.1:{port}")),
        ("content-length", "4".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    assert_eq!(headers, expected);
    assert_eq!(sent, body);
}
