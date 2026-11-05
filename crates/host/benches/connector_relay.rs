//! `connector_relay`: 1,000 `tools/list` lines through the case-law relay to S28's fake
//! connector, which answers at once. The figure is the relay's own time per message, timed
//! inside the relay (its line read to the POST's start, plus the reply to its last line
//! written) and reported through `RelayEvents::overhead`: the median, p95 and the slowest, after
//! 50 untimed lines of warm-up. The client speaks the relay protocol as `clauseo-mcp connector`
//! does, which passes each line through unparsed, so the relay reads the same bytes. Wall
//! clock: it reports and never fails a pull request (budget row `connector_relay_overhead_p95`).

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and its client: a failure ends the bench run with its error"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeOptions, ScratchDir};
use clauseo_connectors::{Connectors, MemoryStore, Origin, SecretStore, bind_loopback_exclusive};
use clauseo_host::SessionKey;
use clauseo_host::SystemClock;
use clauseo_host::connectors::{FixedOffset, RelayConfig, RelayEvent, RelayEvents, Relays};

const WARM_UP: u64 = 50;
const MESSAGES: u64 = 1_000;

/// Keeps every overhead figure the relay reports.
#[derive(Default)]
struct Overheads(Mutex<Vec<Duration>>);

impl RelayEvents for Overheads {
    fn event(&self, _event: RelayEvent) {}

    fn overhead(&self, took: Duration) {
        self.0.lock().unwrap().push(took);
    }
}

/// One `tools/list` line and its answer.
fn round_trip(writer: &mut TcpStream, reader: &mut BufReader<TcpStream>, id: u64) {
    let line = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#);
    writer.write_all(line.as_bytes()).unwrap();
    writer.write_all(b"\n").unwrap();
    let mut answer = Vec::new();
    reader.read_until(b'\n', &mut answer).unwrap();
    assert!(answer.starts_with(br"{"), "an answer line");
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[expect(clippy::print_stdout, reason = "the bench's report")]
fn main() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("relay-bench").unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let connectors = Connectors::new(fake.config(scratch.state_file()), store).unwrap();
    let overheads = Arc::new(Overheads::default());
    let relays = Relays::new(RelayConfig {
        connector: connectors.get("case-law").unwrap(),
        origin: Origin::Fake(fake.base_url()),
        day_file: scratch.path().join("connectors").join("day.json"),
        clock: Arc::new(SystemClock),
        local_day: Arc::new(FixedOffset(19_800)),
        events: Arc::clone(&overheads) as Arc<dyn RelayEvents>,
    });
    let key = SessionKey {
        matter: "01900000-0000-7000-8000-000000000007".parse().unwrap(),
        thread: "01900000-0000-7000-8000-000000000008".parse().unwrap(),
        launch: 1,
    };
    let (session, token) = relays
        .open(key, bind_loopback_exclusive(0).unwrap())
        .unwrap();
    let mut writer = TcpStream::connect(session.addr()).unwrap();
    let mut reader = BufReader::new(writer.try_clone().unwrap());
    let hello = format!(
        r#"{{"clauseo_relay":1,"token":"{}","connector":"case-law"}}"#,
        token.expose()
    );
    writer.write_all(hello.as_bytes()).unwrap();
    writer.write_all(b"\n").unwrap();
    let mut answer = String::new();
    reader.read_line(&mut answer).unwrap();
    assert_eq!(answer.trim_end(), r#"{"clauseo_relay":1,"ok":true}"#);
    for id in 0..WARM_UP {
        round_trip(&mut writer, &mut reader, id);
    }
    overheads.0.lock().unwrap().clear();
    for id in WARM_UP..WARM_UP + MESSAGES {
        round_trip(&mut writer, &mut reader, id);
    }
    // The relay closes once its last answer is out and reported.
    writer.shutdown(std::net::Shutdown::Write).unwrap();
    reader.read_to_end(&mut Vec::new()).unwrap();
    let mut figures = overheads.0.lock().unwrap().clone();
    figures.sort_unstable();
    assert_eq!(u64::try_from(figures.len()).unwrap(), MESSAGES);
    let at = |percent: usize| figures[(figures.len() * percent).div_ceil(100).saturating_sub(1)];
    println!(
        "connector_relay: {MESSAGES} tools/list lines, the relay's own time per message: \
         median {:.3} ms, p95 {:.3} ms, slowest {:.3} ms",
        millis(at(50)),
        millis(at(95)),
        millis(figures[figures.len() - 1]),
    );
}
