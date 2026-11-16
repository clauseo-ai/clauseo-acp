//! `connector_relay`: lines through the case-law relay to S28's fake connector, which answers at
//! once. The figure is the relay's own time per message, timed inside the relay (its line read
//! to the POST's start, plus the reply to its last line written) and reported through
//! `RelayEvents::overhead`: the median, p95 and the slowest, after untimed warm-up lines. The
//! client speaks the relay protocol as `clauseo-mcp connector` does, which passes each line
//! through unparsed, so the relay reads the same bytes. Wall clock: it reports and never fails a
//! pull request (budget row `connector_relay_overhead_p95`). Three runs:
//!
//! 1. 1,000 `tools/list` lines on one connection: the relay alone.
//! 2. 500 paid `tools/call`s (`search_law`, with an API key) on one connection: the caps and the
//!    `day.json` write with its fsyncs, which the call waits for before it goes out. A new turn
//!    every 50 calls keeps the turn cap away; each run has its own `day.json`, under the day cap.
//! 3. 400 paid `tools/call`s, 16 in flight (4 connections × 4): the same, with the writes of
//!    concurrent calls meeting; also the wall time per call.

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and its client: a failure ends the bench run with its error"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
use clauseo_connectors::{
    Connectors, MemoryStore, Origin, Secret, SecretStore, bind_loopback_exclusive,
};
use clauseo_host::connectors::{
    FixedOffset, RelayConfig, RelayEvent, RelayEvents, RelaySession, Relays,
};
use clauseo_host::{Clock as _, SessionKey, SystemClock};

const WARM_UP: u64 = 50;
const MESSAGES: u64 = 1_000;
const PAID_WARM_UP: u64 = 20;
const PAID: u64 = 500;
/// Calls between two turn starts: under the turn cap of 60.
const PER_TURN: u64 = 48;
const CONNECTIONS: u64 = 4;
const IN_FLIGHT: u64 = 4;
const CONCURRENT_ROUNDS: u64 = 25;

/// Keeps every overhead figure the relay reports.
#[derive(Default)]
struct Overheads(Mutex<Vec<Duration>>);

impl RelayEvents for Overheads {
    fn event(&self, _event: RelayEvent) {}

    fn overhead(&self, took: Duration) {
        self.0.lock().unwrap().push(took);
    }
}

/// One relay session on the fake, with its own data folder (and so its own `day.json`).
struct Bench {
    _fake: FakeHandle,
    _scratch: ScratchDir,
    _connectors: Connectors,
    _relays: Relays,
    overheads: Arc<Overheads>,
    session: RelaySession,
    token: Secret,
}

impl Bench {
    /// With `paid`, an API key is added (as PR3's tests sign in) and the balance is ample.
    fn start(paid: bool) -> Bench {
        let fake = FakeConnector::start(FakeOptions::default()).unwrap();
        let scratch = ScratchDir::new("relay-bench").unwrap();
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let connectors = Connectors::new(fake.config(scratch.state_file()), store).unwrap();
        let handle = connectors.get("case-law").unwrap();
        if paid {
            fake.set_balance(i64::MAX / 2);
            handle.add_api_key(&fake.new_api_key()).unwrap();
        }
        let overheads = Arc::new(Overheads::default());
        let relays = Relays::new(RelayConfig {
            connector: handle,
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
        Bench {
            _fake: fake,
            _scratch: scratch,
            _connectors: connectors,
            _relays: relays,
            overheads,
            session,
            token,
        }
    }

    /// A connection past its hello.
    fn connect(&self) -> Conn {
        let writer = TcpStream::connect(self.session.addr()).unwrap();
        let mut conn = Conn {
            reader: BufReader::new(writer.try_clone().unwrap()),
            writer,
        };
        let hello = format!(
            r#"{{"clauseo_relay":1,"token":"{}","connector":"case-law"}}"#,
            self.token.expose()
        );
        conn.send(&hello);
        assert_eq!(conn.recv(), r#"{"clauseo_relay":1,"ok":true}"#);
        conn
    }

    /// Ends `conns` and waits until the relay has reported every answer, then takes the
    /// figures, sorted.
    fn finish(&self, conns: Vec<Conn>) -> Vec<Duration> {
        for mut conn in conns {
            // The relay closes once its last answer is out and reported.
            conn.writer.shutdown(std::net::Shutdown::Write).unwrap();
            conn.reader.read_to_end(&mut Vec::new()).unwrap();
        }
        let mut figures = std::mem::take(&mut *self.overheads.0.lock().unwrap());
        figures.sort_unstable();
        figures
    }

    fn forget_warm_up(&self) {
        self.overheads.0.lock().unwrap().clear();
    }
}

struct Conn {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Conn {
    fn send(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).unwrap();
        self.writer.write_all(b"\n").unwrap();
    }

    fn recv(&mut self) -> String {
        let mut answer = String::new();
        self.reader.read_line(&mut answer).unwrap();
        answer.trim_end().to_owned()
    }

    fn list(&mut self, id: u64) {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#
        ));
    }

    fn paid(&mut self, id: u64) {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"search_law","arguments":{{"query":"section 138 NI Act"}}}}}}"#
        ));
    }

    /// A paid call's answer: a tool result that is not an error (no cap met).
    fn recv_paid(&mut self) {
        let answer: serde_json::Value = serde_json::from_str(&self.recv()).unwrap();
        assert_eq!(answer["result"]["isError"], false, "{answer}");
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// `name`'s median, p95 and slowest, as a report line.
fn report(name: &str, figures: &[Duration]) -> String {
    let at = |percent: usize| figures[(figures.len() * percent).div_ceil(100).saturating_sub(1)];
    format!(
        "connector_relay: {name}, the relay's own time per message: median {:.3} ms, \
         p95 {:.3} ms, slowest {:.3} ms",
        millis(at(50)),
        millis(at(95)),
        millis(figures[figures.len() - 1]),
    )
}

fn tools_list() -> String {
    let bench = Bench::start(false);
    let mut conn = bench.connect();
    let mut round_trip = |id: u64| {
        conn.list(id);
        assert!(conn.recv().starts_with('{'), "an answer line");
    };
    for id in 0..WARM_UP {
        round_trip(id);
    }
    bench.forget_warm_up();
    for id in WARM_UP..WARM_UP + MESSAGES {
        round_trip(id);
    }
    let figures = bench.finish(vec![conn]);
    assert_eq!(u64::try_from(figures.len()).unwrap(), MESSAGES);
    report(&format!("{MESSAGES} tools/list lines"), &figures)
}

fn paid_sequential() -> String {
    let bench = Bench::start(true);
    let turns = bench.session.turns();
    let mut conn = bench.connect();
    for id in 0..PAID_WARM_UP + PAID {
        if id == PAID_WARM_UP {
            bench.forget_warm_up();
        }
        if id.is_multiple_of(PER_TURN) {
            turns.start();
        }
        conn.paid(id);
        conn.recv_paid();
    }
    let figures = bench.finish(vec![conn]);
    assert_eq!(u64::try_from(figures.len()).unwrap(), PAID);
    report(&format!("{PAID} paid tools/calls, one at a time"), &figures)
}

fn paid_concurrent() -> String {
    let bench = Bench::start(true);
    let turns = bench.session.turns();
    let mut conns: Vec<Conn> = (0..CONNECTIONS).map(|_| bench.connect()).collect();
    let clock = SystemClock;
    let started = clock.now().mono;
    let mut id = 0u64;
    for round in 0..CONCURRENT_ROUNDS {
        if (round * CONNECTIONS * IN_FLIGHT).is_multiple_of(PER_TURN) {
            turns.start();
        }
        for conn in &mut conns {
            for _ in 0..IN_FLIGHT {
                conn.paid(id);
                id += 1;
            }
        }
        for conn in &mut conns {
            for _ in 0..IN_FLIGHT {
                conn.recv_paid();
            }
        }
    }
    let wall = clock.now().mono.saturating_sub(started);
    let figures = bench.finish(conns);
    assert_eq!(u64::try_from(figures.len()).unwrap(), id);
    let in_flight = CONNECTIONS * IN_FLIGHT;
    let figures = report(
        &format!("{id} paid tools/calls, {in_flight} in flight"),
        &figures,
    );
    format!(
        "{figures}\nconnector_relay: {id} paid tools/calls, {in_flight} in flight: wall time per \
         call {:.3} ms",
        millis(wall) / f64::from(u32::try_from(id).unwrap()),
    )
}

#[expect(clippy::print_stdout, reason = "the bench's report")]
fn main() {
    for report in [tools_list(), paid_sequential(), paid_concurrent()] {
        println!("{report}");
    }
}
