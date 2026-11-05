//! Unit tests for the connector mode: a test TCP listener stands in for the relay, and the mode
//! runs in-process with its environment, stdin and stdout given. The binary is tested in
//! `tests/connector.rs`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Cursor, ErrorKind, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use super::line::{self, Line};
use super::{MAX_LINE_BYTES, Settings, USAGE, run_with};
use crate::write;

/// A 64-character lowercase token. Its content must never reach stdout, stderr or a failure.
pub(super) const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// The answer that accepts the handshake.
pub(super) const OK: &str = r#"{"clauseo_relay":1,"ok":true}"#;

/// Collects what the mode writes to stdout.
#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub(super) fn out(&self) -> super::line::Out {
        Arc::new(Mutex::new(self.clone()))
    }

    pub(super) fn text(&self) -> String {
        let bytes = self.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    pub(super) fn replies(&self) -> Vec<Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A bad-setting case: the command line and the environment it runs with.
type Case = (Vec<&'static str>, Vec<(&'static str, OsString)>);

/// A scratch folder, removed on drop.
pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-mcp-connector-{test}-{}",
            std::process::id()
        ));
        if path.exists() {
            write::remove_dir_all(&path).unwrap();
        }
        write::create_dir(&path).unwrap();
        Self(path)
    }

    /// A file in the folder holding `content`.
    pub(super) fn file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        write::write(&path, content).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = write::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}

/// The environment the mode sees: the given variables, nothing else.
pub(super) fn env_of(vars: &[(&'static str, OsString)]) -> BTreeMap<&'static str, OsString> {
    vars.iter().cloned().collect()
}

/// `CLAUSEO_RELAY` for a listener on `port`, and the token file at `file`.
pub(super) fn good_env(port: u16, file: &Path) -> Vec<(&'static str, OsString)> {
    vec![
        ("CLAUSEO_RELAY", format!("127.0.0.1:{port}").into()),
        ("CLAUSEO_RELAY_TOKEN_FILE", file.into()),
    ]
}

/// Runs the mode with `name` and `vars`, stdin `input`, and returns the result with what it wrote.
pub(super) fn run_mode(
    vars: &[(&'static str, OsString)],
    name: &[&str],
    input: &[u8],
) -> (Result<i32, crate::Failure>, Capture) {
    let map = env_of(vars);
    let lookup = |key: &str| map.get(key).cloned();
    let args: Vec<OsString> = name.iter().map(|arg| OsString::from(*arg)).collect();
    let capture = Capture::default();
    let result = run_with(&args, &lookup, Cursor::new(input.to_vec()), &capture.out());
    (result, capture)
}

/// Polls of the listener before a test relay gives up: 2000 of 5 ms, about 10 s.
const ACCEPT_POLLS: u32 = 2000;
/// The wait between two polls of the listener.
const ACCEPT_POLL: Duration = Duration::from_millis(5);
/// How long a test relay's read waits before it fails, naming what it waited for: a mode that
/// keeps its socket open without writing fails the test here, not at the runner's 30 s kill.
const READ_LIMIT: Duration = Duration::from_secs(10);

/// Waits for `after`, without `thread::sleep` (`clippy.toml` bans it). The channel's sender is
/// held, so nothing wakes the wait early.
pub(super) fn pause(after: Duration) {
    let (_hold, wake) = mpsc::channel::<()>();
    if wake.recv_timeout(after).is_err() {
        // The time is up, as the sender is still held.
    }
}

/// Accepts the mode's connection, or gives up after about 10 s and returns `None`. A mode that
/// never connects (a refused token, say) then fails its test with its reply, instead of
/// blocking the test in `accept` until the runner kills it. The stream is blocking again, with
/// reads bounded by [`READ_LIMIT`].
pub(super) fn accept_within_limit(listener: &TcpListener) -> Option<TcpStream> {
    listener.set_nonblocking(true).unwrap();
    for _ in 0..ACCEPT_POLLS {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(READ_LIMIT)).unwrap();
                return Some(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => pause(ACCEPT_POLL),
            Err(error) => panic!("accept failed: {error}"),
        }
    }
    None
}

/// Accepts one connection, reads the handshake line, answers `answer` (`None`: closes without
/// answering), then runs `then` with the reader and a writer on the connection. Returns the
/// handshake line and what `then` returned. If the mode never connects, the thread panics
/// after about 10 s, and the test fails with that message.
pub(super) fn relay<R: Send + 'static>(
    listener: TcpListener,
    answer: Option<&'static str>,
    then: impl FnOnce(&mut BufReader<TcpStream>, &TcpStream) -> R + Send + 'static,
) -> JoinHandle<(String, R)> {
    thread::spawn(move || {
        let Some(stream) = accept_within_limit(&listener) else {
            panic!("the mode never connected to the relay within 10 seconds");
        };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut hello = String::new();
        reader
            .read_line(&mut hello)
            .expect("the relay read no hello within 10 seconds of the connection");
        match answer {
            Some(text) => {
                let mut writer = &stream;
                writer.write_all(text.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
            }
            None => stream.shutdown(Shutdown::Both).unwrap(),
        }
        let result = then(&mut reader, &stream);
        (hello, result)
    })
}

/// Every line `reader` reads until the connection ends, as text with the newline kept.
pub(super) fn lines_until_end(reader: &mut BufReader<TcpStream>) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .expect("the relay read nothing for 10 seconds: the mode kept the connection open");
        if read == 0 {
            return lines;
        }
        lines.push(line);
    }
}

/// A port with nothing listening on it: bound, then dropped.
pub(super) fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub(super) fn token_file(scratch: &Scratch, content: &str) -> PathBuf {
    scratch.file("relay-token", content)
}

pub(super) fn assert_no_token(capture: &Capture) {
    assert!(!capture.text().contains(TOKEN), "stdout holds the token");
}

#[test]
fn the_handshake_carries_the_file_token_and_lines_pass_unchanged() {
    let scratch = Scratch::new("pass");
    let file = token_file(&scratch, &format!("{TOKEN}\n"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = relay(listener, Some(OK), |reader, writer| {
        let seen = lines_until_end(reader);
        // Replies go back byte for byte, including odd spacing and a duplicate key.
        let mut writer = writer;
        writer
            .write_all(b"{\"id\":1,\"result\":{}}\n{\"id\":2,  \"result\": {\"a\":1,\"a\":2}}\n")
            .unwrap();
        seen
    });
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        "\n",
        "{\"id\":  2,\"id\":3,  \"method\" :\"tools/call\"}\n",
    );
    let (result, capture) = run_mode(&good_env(port, &file), &["case-law"], input.as_bytes());
    assert_eq!(result.unwrap(), 0);
    let (hello, seen) = server.join().unwrap();
    assert_eq!(
        hello,
        format!("{{\"clauseo_relay\":1,\"token\":\"{TOKEN}\",\"connector\":\"case-law\"}}\n")
    );
    assert_eq!(
        seen,
        [
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
            "{\"id\":  2,\"id\":3,  \"method\" :\"tools/call\"}\n",
        ]
    );
    // Request 3 had no reply from the relay; the drop after stdin closed answers it.
    let replies = capture.text();
    assert!(
        replies
            .starts_with("{\"id\":1,\"result\":{}}\n{\"id\":2,  \"result\": {\"a\":1,\"a\":2}}\n"),
        "{replies}"
    );
    let last = capture.replies().pop().unwrap();
    assert_eq!(last["id"], 3);
    assert_eq!(last["error"]["data"]["reason"], "connect_failed");
    assert_no_token(&capture);
}

#[test]
fn a_line_at_the_cap_is_forwarded_and_one_over_it_is_refused() {
    let scratch = Scratch::new("cap");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = relay(listener, Some(OK), |reader, _| lines_until_end(reader));
    let mut input = vec![b'x'; MAX_LINE_BYTES];
    input.push(b'\n');
    input.extend(vec![b'x'; MAX_LINE_BYTES + 1]);
    input.push(b'\n');
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n");
    let (result, capture) = run_mode(&good_env(port, &file), &["case-law"], &input);
    assert_eq!(result.unwrap(), 0);
    let (_, seen) = server.join().unwrap();
    // The line at the cap went to the relay; the one over it did not, and ended the link.
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].len(), MAX_LINE_BYTES + 1);
    let replies = capture.replies();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0]["id"], Value::Null);
    assert_eq!(replies[0]["error"]["code"], -32600);
    assert_eq!(
        replies[0]["error"]["message"],
        "Line too long for Clauseo's case-law relay."
    );
    assert_eq!(replies[1]["id"], 9);
    assert_eq!(replies[1]["error"]["data"]["reason"], "connect_failed");
    assert_no_token(&capture);
}

#[test]
fn with_no_relay_each_request_gets_not_reachable_and_notifications_nothing() {
    let scratch = Scratch::new("no-relay");
    let file = token_file(&scratch, TOKEN);
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{}}\n",
        "not json\n",
        "[{\"jsonrpc\":\"2.0\",\"id\":\"a\",\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"method\":\"x\"}]\n",
    );
    let (result, capture) = run_mode(
        &good_env(closed_port(), &file),
        &["case-law"],
        input.as_bytes(),
    );
    assert_eq!(result.unwrap(), 0);
    let replies = capture.replies();
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[0]["id"], 7);
    assert_eq!(replies[0]["jsonrpc"], "2.0");
    assert_eq!(replies[0]["error"]["code"], -32000);
    assert_eq!(
        replies[0]["error"]["message"],
        "Case law is not available in this conversation: Clauseo's relay is not reachable. Tell the lawyer to start a new conversation."
    );
    assert_eq!(replies[0]["error"]["data"]["reason"], "connect_failed");
    assert_eq!(replies[1]["id"], Value::Null);
    assert_eq!(replies[1]["error"]["code"], -32700);
    assert_eq!(replies[2]["id"], "a");
    assert_eq!(replies[2]["error"]["data"]["reason"], "connect_failed");
    assert_no_token(&capture);
}

#[test]
fn a_refused_handshake_gives_the_hosts_code_as_the_reason() {
    // A plain code is passed on; anything else, or no answer at all, is `connect_failed`.
    let cases: [(&'static str, &str); 4] = [
        (
            r#"{"clauseo_relay":1,"ok":false,"error":"bad_token"}"#,
            "bad_token",
        ),
        (
            r#"{"clauseo_relay":1,"ok":false,"error":"Not A Code!"}"#,
            "connect_failed",
        ),
        (r#"{"clauseo_relay":1,"ok":false}"#, "connect_failed"),
        ("not json", "connect_failed"),
    ];
    for (answer, reason) in cases {
        let scratch = Scratch::new("refused");
        let file = token_file(&scratch, TOKEN);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = relay(listener, Some(answer), |reader, _| lines_until_end(reader));
        let input = "{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"tools/list\"}\n";
        let (result, capture) = run_mode(&good_env(port, &file), &["case-law"], input.as_bytes());
        assert_eq!(result.unwrap(), 0);
        let (_, seen) = server.join().unwrap();
        assert!(seen.is_empty(), "a refused link forwarded {seen:?}");
        let replies = capture.replies();
        assert_eq!(replies.len(), 1, "{answer}: {replies:?}");
        assert_eq!(replies[0]["id"], "x");
        assert_eq!(replies[0]["error"]["data"]["reason"], reason, "{answer}");
        assert_no_token(&capture);
    }
}

#[test]
fn a_handshake_that_gets_no_answer_is_connect_failed() {
    let scratch = Scratch::new("silent");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = relay(listener, None, |_, _| ());
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    let (result, capture) = run_mode(&good_env(port, &file), &["case-law"], input.as_bytes());
    assert_eq!(result.unwrap(), 0);
    server.join().unwrap();
    let replies = capture.replies();
    assert_eq!(replies[0]["error"]["data"]["reason"], "connect_failed");
    assert_no_token(&capture);
}

#[test]
fn the_token_file_takes_surrounding_whitespace() {
    let scratch = Scratch::new("settings-ok");
    let file = token_file(&scratch, &format!("{TOKEN}\r\n"));
    let port = closed_port();
    let map = env_of(&good_env(port, &file));
    let lookup = |key: &str| map.get(key).cloned();
    let settings = Settings::parse(&[OsString::from("case-law")], &lookup);
    let settings = settings.unwrap_or_else(|reason| panic!("refused: {reason}"));
    assert_eq!(settings.token, TOKEN);
    assert_eq!(settings.name, "case-law");
    assert_eq!(settings.addr.port(), port);
    for relay in ["127.0.0.1:1", "127.0.0.1:65535"] {
        let vars = [
            ("CLAUSEO_RELAY", OsString::from(relay)),
            ("CLAUSEO_RELAY_TOKEN_FILE", file.clone().into()),
        ];
        let map = env_of(&vars);
        let lookup = |key: &str| map.get(key).cloned();
        assert!(
            Settings::parse(&[OsString::from("a1")], &lookup).is_ok(),
            "{relay}"
        );
    }
}

#[test]
fn every_bad_setting_exits_2_with_one_usage_line_and_no_token() {
    let scratch = Scratch::new("settings-bad");
    let good = token_file(&scratch, TOKEN);
    let upper = token_file(&scratch, &TOKEN.to_uppercase());
    let short = token_file(&scratch, &TOKEN[1..]);
    let long = token_file(&scratch, &format!("{TOKEN}0"));
    let not_hex = token_file(&scratch, &TOKEN.replacen('a', "g", 1));
    let padded = token_file(&scratch, &format!(" {TOKEN}"));
    let empty = token_file(&scratch, "");
    let directory = scratch.0.clone();
    let missing = scratch.0.join("missing");
    let relay = |value: &str| {
        vec![
            ("CLAUSEO_RELAY", OsString::from(value)),
            ("CLAUSEO_RELAY_TOKEN_FILE", good.clone().into()),
        ]
    };
    let with_file = |path: &Path| {
        vec![
            ("CLAUSEO_RELAY", OsString::from("127.0.0.1:61000")),
            ("CLAUSEO_RELAY_TOKEN_FILE", path.into()),
        ]
    };
    let cases: Vec<Case> = vec![
        (vec![], relay("127.0.0.1:61000")),
        (vec!["Case-Law"], relay("127.0.0.1:61000")),
        (vec!["case-law", "extra"], relay("127.0.0.1:61000")),
        (vec!["-case"], relay("127.0.0.1:61000")),
        (vec!["case_law"], relay("127.0.0.1:61000")),
        (
            vec!["case-law"],
            vec![("CLAUSEO_RELAY_TOKEN_FILE", good.clone().into())],
        ),
        (vec!["case-law"], relay("localhost:61000")),
        (vec!["case-law"], relay("0.0.0.0:61000")),
        (vec!["case-law"], relay("10.0.0.1:61000")),
        (vec!["case-law"], relay("127.0.0.1:0")),
        (vec!["case-law"], relay("127.0.0.1:065000")),
        (vec!["case-law"], relay("127.0.0.1:65536")),
        (vec!["case-law"], relay("127.0.0.1:")),
        (vec!["case-law"], relay("127.0.0.1:61000 ")),
        (vec!["case-law"], relay("[::1]:61000")),
        (
            vec!["case-law"],
            vec![("CLAUSEO_RELAY", OsString::from("127.0.0.1:61000"))],
        ),
        (vec!["case-law"], with_file(&missing)),
        (vec!["case-law"], with_file(&directory)),
        (vec!["case-law"], with_file(&upper)),
        (vec!["case-law"], with_file(&short)),
        (vec!["case-law"], with_file(&long)),
        (vec!["case-law"], with_file(&not_hex)),
        (vec!["case-law"], with_file(&padded)),
        (vec!["case-law"], with_file(&empty)),
    ];
    let secrets = [
        TOKEN.to_uppercase(),
        TOKEN[1..].to_owned(),
        format!("{TOKEN}0"),
        TOKEN.replacen('a', "g", 1),
    ];
    for (name, vars) in cases {
        let (result, capture) = run_mode(&vars, &name, b"{\"id\":1,\"method\":\"ping\"}\n");
        let failure = match result {
            Err(failure) => failure,
            Ok(code) => panic!("{name:?} {vars:?}: exit {code}"),
        };
        assert_eq!(failure.code, 2, "{name:?}");
        assert!(failure.message.starts_with(USAGE), "{}", failure.message);
        assert!(!failure.message.contains('\n'), "{}", failure.message);
        assert!(!failure.message.contains(TOKEN), "{}", failure.message);
        for secret in &secrets {
            assert!(
                !failure.message.contains(secret.as_str()),
                "{}",
                failure.message
            );
        }
        assert_eq!(
            capture.text(),
            "",
            "{name:?}: a refused setting wrote to stdout"
        );
    }
}

#[test]
fn the_name_and_the_address_are_checked_exactly() {
    let names = ["case-law", "a", "a1-b2", "0-x"];
    for name in names {
        assert!(super::valid_name(name), "{name}");
    }
    for name in [
        "",
        "Case",
        "case_law",
        "-case",
        "case law",
        "ÿ",
        &"a".repeat(33),
    ] {
        assert!(!super::valid_name(name), "{name}");
    }
    assert!(super::relay_addr(&OsString::from("127.0.0.1:1")).is_some());
    for value in [
        "127.0.0.1:00",
        "127.0.0.1:+1",
        "127.0.0.1:1x",
        "127.0.0.1:1.0",
    ] {
        assert!(
            super::relay_addr(&OsString::from(value)).is_none(),
            "{value}"
        );
    }
}

#[test]
fn a_read_line_has_a_cap_and_a_long_line_is_skipped_to_its_end() {
    let mut text = Cursor::new(b"ab\ncde\r\nxxxxx\nf".to_vec());
    assert_eq!(
        line::read(&mut text, 4).unwrap(),
        Line::Text(b"ab".to_vec())
    );
    assert_eq!(
        line::read(&mut text, 4).unwrap(),
        Line::Text(b"cde\r".to_vec())
    );
    assert_eq!(line::read(&mut text, 4).unwrap(), Line::TooLong);
    // The next line starts where the long one ended.
    assert_eq!(line::read(&mut text, 4).unwrap(), Line::Text(b"f".to_vec()));
    assert_eq!(line::read(&mut text, 4).unwrap(), Line::Eof);
    let mut edge = Cursor::new(b"abc\nabcd".to_vec());
    assert_eq!(
        line::read(&mut edge, 3).unwrap(),
        Line::Text(b"abc".to_vec())
    );
    assert_eq!(line::read(&mut edge, 3).unwrap(), Line::TooLong);
    let mut empty = Cursor::new(b"\n".to_vec());
    assert_eq!(line::read(&mut empty, 3).unwrap(), Line::Text(Vec::new()));
}
