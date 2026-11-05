//! The relay against S28's fake connector: the rig, an agent-side client speaking the relay
//! protocol (as `clauseo-mcp connector` does, byte for byte), the events recorder, a log
//! capture, and a gated upstream whose answers the test releases one by one.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{ConnectorHandle, Connectors, MemoryStore, Origin, Secret, SecretStore};
use clauseo_protocol::{MatterId, ThreadId, Timestamp};
use serde_json::{Value, json};

use super::super::super::{FixedOffset, RelayEvent, RelayEvents};
use super::super::{RelayConfig, RelaySession, Relays};
use crate::ManualClock;
use crate::config::SessionKey;
use crate::pipeline::tests::support::id;

/// India's offset from UTC.
pub(super) const INDIA_OFFSET: i32 = 19_800;
/// 2026-11-02 00:00 UTC, 05:30 in India.
pub(super) const START_MILLIS: i64 = 1_793_577_600_000;

/// Everything a relay test needs.
pub(super) struct Rig {
    pub(super) fake: FakeHandle,
    pub(super) scratch: ScratchDir,
    pub(super) _connectors: Connectors,
    pub(super) handle: ConnectorHandle,
    pub(super) clock: ManualClock,
    pub(super) events: Arc<Recorder>,
    pub(super) relays: Relays,
    pub(super) session: RelaySession,
    pub(super) token: Secret,
    pub(super) key: SessionKey,
}

/// The session every rig opens first.
pub(super) fn key(launch: u64) -> SessionKey {
    SessionKey {
        matter: id::<MatterId>(7),
        thread: id::<ThreadId>(8),
        launch,
    }
}

/// A rig on the fake, with nothing added.
pub(super) fn rig() -> Rig {
    rig_with(&RigOptions::default())
}

#[derive(Default)]
pub(super) struct RigOptions {
    /// Forward to this origin instead of the fake (the gated upstream).
    pub(super) upstream: Option<String>,
    /// `day.json`'s text before the relays start.
    pub(super) day_json: Option<String>,
    /// The log capture the relay's threads write to.
    pub(super) log: Option<tracing::Dispatch>,
}

pub(super) fn rig_with(options: &RigOptions) -> Rig {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("relay").unwrap();
    let mut config = fake.config(scratch.state_file());
    if let Some(upstream) = &options.upstream {
        config.origin = Origin::Fake(url::Url::parse(upstream).unwrap());
    }
    let origin = config.origin.clone();
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let connectors = Connectors::new(config, store).unwrap();
    let handle = connectors.get("case-law").unwrap();
    let day_file = scratch.path().join("connectors").join("day.json");
    if let Some(text) = &options.day_json {
        crate::write::create_dir_all(day_file.parent().unwrap()).unwrap();
        crate::write::write_file(&day_file, text.as_bytes()).unwrap();
    }
    let clock = ManualClock::new(Timestamp::new(START_MILLIS).unwrap());
    let events = Arc::new(Recorder::default());
    let relays = Relays::new(RelayConfig {
        connector: handle.clone(),
        origin,
        day_file,
        clock: Arc::new(clock.clone()),
        local_day: Arc::new(FixedOffset(INDIA_OFFSET)),
        events: Arc::clone(&events) as Arc<dyn RelayEvents>,
    });
    let open = || relays.open(key(1), loopback());
    let (session, token) = match &options.log {
        Some(dispatch) => tracing::dispatcher::with_default(dispatch, open),
        None => open(),
    }
    .unwrap();
    Rig {
        fake,
        scratch,
        _connectors: connectors,
        handle,
        clock,
        events,
        relays,
        session,
        token,
        key: key(1),
    }
}

/// A fresh loopback listener.
pub(super) fn loopback() -> TcpListener {
    clauseo_connectors::bind_loopback_exclusive(0).unwrap()
}

impl Rig {
    /// A connection past its handshake.
    pub(super) fn agent(&self) -> Agent {
        let mut agent = Agent::connect(&self.session);
        assert_eq!(
            agent.hello(self.token.expose(), "case-law"),
            Some(json!({"clauseo_relay": 1, "ok": true}))
        );
        agent
    }

    /// Signs in through the fake's whole OAuth flow.
    pub(super) fn sign_in(&self) {
        let pending = self.handle.begin_sign_in(None).unwrap();
        let url = pending.authorize_url().clone();
        let finisher = std::thread::spawn(move || pending.finish());
        let page = self.fake.drive_authorize(&url).unwrap();
        assert_eq!(page.status, 200);
        finisher.join().unwrap().unwrap();
    }

    /// Adds a new API key the fake accepts, and returns it.
    pub(super) fn add_api_key(&self) -> String {
        let key = self.fake.new_api_key();
        self.handle.add_api_key(&key).unwrap();
        key
    }

    /// The MCP requests the fake received, as JSON.
    pub(super) fn mcp_requests(&self) -> Vec<clauseo_connectors::fake::RecordedRequest> {
        self.fake.requests_to("/mcp/case-law")
    }

    /// The paid calls `day.json` holds; 0 when there is no file.
    pub(super) fn paid_today(&self) -> u64 {
        std::fs::read(self.day_file()).map_or(0, |bytes| {
            serde_json::from_slice::<Value>(&bytes).unwrap()["paid"]
                .as_u64()
                .unwrap()
        })
    }

    pub(super) fn day_file(&self) -> PathBuf {
        self.scratch.path().join("connectors").join("day.json")
    }
}

/// The agent's side of one connection.
pub(super) struct Agent {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Agent {
    pub(super) fn connect(session: &RelaySession) -> Agent {
        let stream = TcpStream::connect(session.addr()).unwrap();
        // A safety net against a hung test, not a wait: every read below is answered sooner.
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        Agent {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    /// Sends a hello and reads the answer; `None` when the relay closed unanswered.
    pub(super) fn hello(&mut self, token: &str, connector: &str) -> Option<Value> {
        self.send(&format!(
            r#"{{"clauseo_relay":1,"token":"{token}","connector":"{connector}"}}"#
        ));
        self.recv_line()
            .map(|line| serde_json::from_slice(&line).unwrap())
    }

    /// Writes `line` and a newline.
    pub(super) fn send(&mut self, line: &str) {
        self.send_bytes(line.as_bytes());
    }

    pub(super) fn send_bytes(&mut self, line: &[u8]) {
        // A relay that closed may refuse the bytes; the read that follows says so.
        let _sent = self
            .writer
            .write_all(line)
            .and_then(|()| self.writer.write_all(b"\n"));
    }

    /// Writes `bytes` as they are, with no newline.
    pub(super) fn send_raw(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
    }

    /// Whether the connection is still open with nothing to read, looked at without waiting.
    pub(super) fn open_and_quiet(&self) -> bool {
        let stream = self.reader.get_ref();
        stream.set_nonblocking(true).unwrap();
        let mut byte = [0u8; 1];
        let open = matches!(stream.peek(&mut byte), Err(err) if err.kind() == std::io::ErrorKind::WouldBlock);
        stream.set_nonblocking(false).unwrap();
        open
    }

    pub(super) fn send_value(&mut self, value: &Value) {
        self.send(&value.to_string());
    }

    /// One line, or `None` at the end of the stream.
    pub(super) fn recv_line(&mut self) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        match self.reader.read_until(b'\n', &mut line) {
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                panic!("no line and no close from the relay in 30 s");
            }
            Ok(0) | Err(_) => None,
            Ok(_) => {
                assert_eq!(line.pop(), Some(b'\n'), "a line ends with a newline");
                Some(line)
            }
        }
    }

    /// One line as JSON.
    pub(super) fn recv(&mut self) -> Value {
        let line = self.recv_line().expect("a line from the relay");
        serde_json::from_slice(&line).unwrap()
    }

    /// Whether the relay closed the connection (with nothing more to read).
    pub(super) fn closed(&mut self) -> bool {
        let mut rest = Vec::new();
        match self.reader.read_to_end(&mut rest) {
            Ok(_) => rest.is_empty(),
            Err(err) => {
                let timed_out = matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                );
                !timed_out && rest.is_empty()
            }
        }
    }

    /// Ends the agent's write half, as `clauseo-mcp` does when stdin closes.
    pub(super) fn shut_write(&self) {
        self.writer.shutdown(std::net::Shutdown::Write).unwrap();
    }
}

/// A `tools/call` of `tool` with `id`.
pub(super) fn call(id: u64, tool: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": tool, "arguments": {"query": "section 138 NI Act"}}})
}

/// A request of `method` with `id`.
pub(super) fn request(id: u64, method: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}})
}

/// The text of an `isError` tool result line.
pub(super) fn error_text(reply: &Value) -> &str {
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    reply["result"]["content"][0]["text"].as_str().unwrap()
}

/// What the relay reported.
#[derive(Default)]
pub(super) struct Recorder {
    pub(super) events: Mutex<Vec<RelayEvent>>,
    pub(super) overheads: Mutex<Vec<Duration>>,
}

impl RelayEvents for Recorder {
    fn event(&self, event: RelayEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn overhead(&self, took: Duration) {
        self.overheads.lock().unwrap().push(took);
    }
}

impl Recorder {
    pub(super) fn take(&self) -> Vec<RelayEvent> {
        std::mem::take(&mut self.events.lock().unwrap())
    }
}

/// A log capture: every line the dispatcher's threads write, as text.
#[derive(Clone, Default)]
pub(super) struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    pub(super) fn dispatch(&self) -> tracing::Dispatch {
        let sink = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::Dispatch::new(subscriber)
    }

    pub(super) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An HTTP upstream that holds each MCP request until the test releases its JSON-RPC id, then
/// answers `{"result":{"n":<id>}}`.
pub(super) struct Gated {
    pub(super) origin: String,
    state: Arc<(Mutex<GateState>, Condvar)>,
}

#[derive(Default)]
struct GateState {
    arrived: Vec<i64>,
    released: HashSet<i64>,
    stopped: bool,
}

impl Gated {
    pub(super) fn start() -> Gated {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new((Mutex::new(GateState::default()), Condvar::new()));
        let shared = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                if shared.0.lock().unwrap().stopped {
                    break;
                }
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || serve_gated(stream, &shared));
            }
        });
        Gated { origin, state }
    }

    /// Waits until `count` requests have arrived; their ids in arrival order.
    pub(super) fn wait_arrived(&self, count: usize) -> Vec<i64> {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap();
        while state.arrived.len() < count {
            state = changed.wait(state).unwrap();
        }
        state.arrived.clone()
    }

    pub(super) fn arrived(&self) -> usize {
        self.state.0.lock().unwrap().arrived.len()
    }

    pub(super) fn release(&self, id: i64) {
        self.state.0.lock().unwrap().released.insert(id);
        self.state.1.notify_all();
    }
}

impl Drop for Gated {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stopped = true;
        self.state.1.notify_all();
        let _wake = TcpStream::connect(self.origin.trim_start_matches("http://"));
    }
}

fn serve_gated(stream: TcpStream, state: &(Mutex<GateState>, Condvar)) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end().to_ascii_lowercase();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (lock, changed) = state;
    let mut gate = lock.lock().unwrap();
    gate.arrived.push(id);
    changed.notify_all();
    while !gate.released.contains(&id) && !gate.stopped {
        gate = changed.wait(gate).unwrap();
    }
    drop(gate);
    let body = json!({"jsonrpc": "2.0", "id": id, "result": {"n": id}}).to_string();
    let answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = stream;
    let _written = stream.write_all(answer.as_bytes());
}

/// Two ends of a loopback connection.
pub(super) fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ours = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (theirs, _) = listener.accept().unwrap();
    (ours, theirs)
}
