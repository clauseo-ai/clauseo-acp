//! The version-history listener over real loopback sockets, with a backend whose matter is never
//! reached (the host's own source is tested end to end in `tests/host/versions_tool.rs`): who
//! gets in, what an authenticated client may ask, and the end of the session.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use clauseo_connectors::Secret;
use clauseo_protocol::{CheckpointId, MatterId, ThreadId};
use serde_json::{Value, json};

use super::{MAX_VERSIONS_LINE_BYTES, VersionsRelay};
use crate::SystemClock;
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;
use crate::versions::{
    Logged, MatterRead, Unreachable, VersionsBackend, VersionsConfig, VersionsSource,
};

/// A source whose matter is never reached: every read is `Unavailable`.
struct NoMatter;

impl VersionsSource for NoMatter {
    fn on_matter(&self, _matter: MatterId, _read: MatterRead) -> Result<(), Unreachable> {
        Err(Unreachable)
    }

    fn logged(&self, _matter: MatterId, _versions: &[CheckpointId]) -> Logged {
        Logged::default()
    }
}

fn key(launch: u64) -> SessionKey {
    SessionKey {
        matter: id::<MatterId>(7),
        thread: id::<ThreadId>(8),
        launch,
    }
}

/// A listener for launch `launch` and its token.
fn open(launch: u64) -> (VersionsRelay, Secret) {
    let backend = VersionsBackend::new(VersionsConfig {
        session: key(launch),
        source: Arc::new(NoMatter),
        clock: Arc::new(SystemClock),
        data_dir: std::path::PathBuf::from("unused"),
        stetdoc: None,
    });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    VersionsRelay::open(key(launch), backend, Arc::new(SystemClock), listener).unwrap()
}

/// The agent's side, speaking the relay protocol as `clauseo-mcp connector` does.
struct Agent {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Agent {
    fn connect(relay: &VersionsRelay) -> Agent {
        let stream = TcpStream::connect(("127.0.0.1", relay.port())).unwrap();
        // A safety net against a hung test, not a wait: every read below is answered sooner.
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Agent {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    /// Sends a hello and reads the answer; `None` when the listener closed unanswered.
    fn hello(&mut self, token: &str, connector: &str) -> Option<Value> {
        self.send(&format!(
            r#"{{"clauseo_relay":1,"token":"{token}","connector":"{connector}"}}"#
        ));
        self.recv()
    }

    /// A connection past the handshake with `token`.
    fn signed_in(relay: &VersionsRelay, token: &Secret) -> Agent {
        let mut agent = Agent::connect(relay);
        let answer = agent.hello(token.expose(), "clauseo-versions");
        assert_eq!(answer, Some(json!({"clauseo_relay": 1, "ok": true})));
        agent
    }

    fn send(&mut self, line: &str) {
        // A listener that closed may refuse the bytes; the read that follows says so.
        let _sent = self
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"));
    }

    /// One line as JSON, or `None` at the end of the stream.
    fn recv(&mut self) -> Option<Value> {
        let mut line = Vec::new();
        match self.reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_slice(&line).unwrap()),
        }
    }

    /// Sends `method` with `params` as request `id` and reads one answer.
    fn ask(&mut self, id: u64, method: &str, params: &Value) -> Value {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(&request.to_string());
        self.recv().expect("an answer")
    }

    /// Whether the listener closed the connection with nothing more to read.
    fn closed(&mut self) -> bool {
        let mut rest = Vec::new();
        self.reader.read_to_end(&mut rest).is_ok() && rest.is_empty()
    }
}

const BAD_TOKEN: &str = r#"{"clauseo_relay":1,"ok":false,"error":"bad_token"}"#;

fn bad_token() -> Value {
    serde_json::from_str(BAD_TOKEN).unwrap()
}

/// No token, a wrong one, the case-law relay's shape with another launch's token, and a hello
/// of the wrong shape: one answer, `bad_token`, then the close.
#[test]
fn a_client_without_this_sessions_token_gets_bad_token_and_the_close() {
    let (relay, token) = open(1);
    let (_other, other_token) = open(2);
    let wrong = "0".repeat(64);
    for hello in [
        String::new(),
        "{}".to_owned(),
        format!(r#"{{"clauseo_relay":1,"connector":"clauseo-versions","token":"{wrong}"}}"#),
        format!(
            r#"{{"clauseo_relay":1,"connector":"clauseo-versions","token":"{}"}}"#,
            other_token.expose()
        ),
        format!(
            r#"{{"clauseo_relay":1,"connector":"clauseo-versions","token":"{}","x":1}}"#,
            token.expose()
        ),
    ] {
        let mut agent = Agent::connect(&relay);
        agent.send(&hello);
        assert_eq!(agent.recv(), Some(bad_token()), "{hello}");
        assert!(agent.closed(), "{hello}");
    }
}

/// The right token with the case-law connector's name is past the token, so it hears
/// `wrong_connector`.
#[test]
fn this_sessions_token_for_another_connector_is_wrong_connector() {
    let (relay, token) = open(1);
    let mut agent = Agent::connect(&relay);
    let answer = agent.hello(token.expose(), "case-law");
    assert_eq!(
        answer,
        Some(json!({"clauseo_relay": 1, "ok": false, "error": "wrong_connector"}))
    );
    assert!(agent.closed());
}

/// With the token: `initialize` and `tools/list` answer, the list holds exactly the two tools,
/// and every other method, or another tool, is a JSON-RPC error.
#[test]
fn with_the_token_only_the_two_tools_are_there_and_anything_else_is_an_error() {
    let (relay, token) = open(1);
    let mut agent = Agent::signed_in(&relay, &token);
    let init = agent.ask(1, "initialize", &json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(init["result"]["serverInfo"]["name"], "clauseo-versions");
    let list = agent.ask(2, "tools/list", &json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["list_versions", "read_version"]);
    for (id, method, params) in [
        (3, "resources/list", json!({})),
        (4, "prompts/list", json!({})),
        (5, "resources/read", json!({"uri": "file:///etc/passwd"})),
        (
            6,
            "tools/call",
            json!({"name": "write_file", "arguments": {}}),
        ),
        (7, "sampling/createMessage", json!({})),
    ] {
        let answer = agent.ask(id, method, &params);
        assert_eq!(answer["id"], id, "{method}");
        assert!(answer["error"]["code"].is_i64(), "{method}: {answer}");
        assert!(answer.get("result").is_none(), "{method}: {answer}");
    }
    // A matter that cannot be reached answers the tool's own refusal, not an error.
    let call = json!({"name": "list_versions", "arguments": {}});
    let answer = agent.ask(8, "tools/call", &call);
    assert_eq!(answer["result"]["isError"], true, "{answer}");
}

/// The framing is the listener's: not JSON, a batch, a line over the cap.
#[test]
fn a_line_not_json_a_batch_and_a_line_too_long_get_their_errors() {
    let (relay, token) = open(1);
    let mut agent = Agent::signed_in(&relay, &token);
    agent.send("not json");
    assert_eq!(agent.recv().unwrap()["error"]["code"], -32700);
    agent.send(r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#);
    assert_eq!(agent.recv().unwrap()["error"]["code"], -32600);
    agent.send(&"x".repeat(MAX_VERSIONS_LINE_BYTES + 1));
    let answer = agent.recv().unwrap();
    assert_eq!(answer["error"]["code"], -32600);
    assert_eq!(
        answer["error"]["message"],
        "Line too long for Clauseo's version-history relay."
    );
    assert!(agent.closed());
}

/// `end` revokes the token first: the open connection is closed, and a new one with the same
/// token gets `bad_token`.
#[test]
fn after_the_end_the_token_is_refused_and_connections_are_closed() {
    let (relay, token) = open(1);
    let mut open = Agent::signed_in(&relay, &token);
    relay.end();
    assert!(open.closed());
    let mut late = Agent::connect(&relay);
    assert_eq!(
        late.hello(token.expose(), "clauseo-versions"),
        Some(bad_token())
    );
    assert!(late.closed());
}

/// A listener bound off loopback is refused before it accepts.
#[test]
fn a_listener_off_loopback_is_refused() {
    let Ok(listener) = TcpListener::bind("0.0.0.0:0") else {
        return;
    };
    let backend = VersionsBackend::new(VersionsConfig {
        session: key(1),
        source: Arc::new(NoMatter),
        clock: Arc::new(SystemClock),
        data_dir: std::path::PathBuf::from("unused"),
        stetdoc: None,
    });
    let opened = VersionsRelay::open(key(1), backend, Arc::new(SystemClock), listener);
    assert_eq!(
        opened.err().map(|err| err.kind()),
        Some(std::io::ErrorKind::InvalidInput)
    );
}
