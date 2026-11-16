//! The case-law relay's memory per upstream reply. S28 reads a reply body of up to 8 MB; the relay
//! must not turn it into a parsed tree (a dense `[0,0,…]` costs about 32 bytes per element as a
//! `serde_json::Value`, so one such reply took about 200 MB and 16 in flight reached GBs). A
//! counting global allocator keeps the live heap's peak across every thread of this process:
//! the relay's, the fake connector's and the upstream's.
//!
//! - One reply of 8 MB less 4 KB: the peak above the baseline stays under [`ONE_REPLY_BOUND`].
//! - 16 such replies at once (4 connections × 4 calls, held by a gate until all 16 arrived): the
//!   peak above the baseline stays under 16 × [`ONE_REPLY_BOUND`].
//!
//! The two tests share the allocator's counters, so they take turns through [`SERIAL`].

#[cfg(test)]
mod memory {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, ScratchDir};
    use clauseo_connectors::{
        Connectors, MemoryStore, Origin, Secret, SecretStore, bind_loopback_exclusive,
    };
    use clauseo_host::SessionKey;
    use clauseo_host::SystemClock;
    use clauseo_host::connectors::{
        FixedOffset, RelayConfig, RelayEvent, RelayEvents, RelaySession, Relays,
    };
    use serde_json::Value;

    /// What one reply may add to the live heap: S28's 8 MB read (and its buffer's growth), the
    /// raw copy the relay keeps, and the small reads around them. About 200 MB before the fix.
    const ONE_REPLY_BOUND: usize = 40 * 1024 * 1024;
    /// One reply body: 8 MB less 4 KB, under S28's 8 MB read limit.
    const BODY_BYTES: usize = 8 * 1024 * 1024 - 4096;
    const MIB: usize = 1024 * 1024;

    /// `bytes` in MiB, to a tenth.
    fn mib(bytes: usize) -> String {
        format!("{}.{}", bytes / MIB, bytes % MIB * 10 / MIB)
    }

    /// Counts the live heap and its peak; `realloc` counts the new block before the old one is
    /// freed, so a copying move is not missed (an in-place one is counted high).
    struct Counting;

    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    static SERIAL: Mutex<()> = Mutex::new(());

    fn grow(by: usize) {
        let live = LIVE.fetch_add(by, Ordering::SeqCst).saturating_add(by);
        PEAK.fetch_max(live, Ordering::SeqCst);
    }

    fn shrink(by: usize) {
        LIVE.fetch_sub(by, Ordering::SeqCst);
    }

    #[expect(
        unsafe_code,
        reason = "a global allocator that forwards to the system's"
    )]
    // SAFETY: every call forwards to `System` with the caller's arguments; the counters only
    // observe.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller's contract is `System.alloc`'s.
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() {
                grow(layout.size());
            }
            ptr
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller's contract is `System.alloc_zeroed`'s.
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() {
                grow(layout.size());
            }
            ptr
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract is `System.dealloc`'s.
            unsafe { System.dealloc(ptr, layout) };
            shrink(layout.size());
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            // SAFETY: the caller's contract is `System.realloc`'s.
            let moved = unsafe { System.realloc(ptr, layout, new_size) };
            if !moved.is_null() {
                grow(new_size);
                shrink(layout.size());
            }
            moved
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    /// Starts a peak measurement: the live heap now is the baseline.
    fn baseline() -> usize {
        let live = LIVE.load(Ordering::SeqCst);
        PEAK.store(live, Ordering::SeqCst);
        live
    }

    /// The peak since [`baseline`], above it.
    fn peak_above(baseline: usize) -> usize {
        PEAK.load(Ordering::SeqCst).saturating_sub(baseline)
    }

    struct Quiet;

    impl RelayEvents for Quiet {
        fn event(&self, _event: RelayEvent) {}
        fn overhead(&self, _took: Duration) {}
    }

    /// The relay on the fake's config, forwarding to `upstream` when given.
    struct Rig {
        fake: FakeHandle,
        _scratch: ScratchDir,
        _connectors: Connectors,
        _relays: Relays,
        session: RelaySession,
        token: Secret,
    }

    fn rig(upstream: Option<&str>) -> Rig {
        let fake = FakeConnector::start(FakeOptions::default()).unwrap();
        let scratch = ScratchDir::new("relay-memory").unwrap();
        let mut config = fake.config(scratch.state_file());
        if let Some(upstream) = upstream {
            config.origin = Origin::Fake(url::Url::parse(upstream).unwrap());
        }
        let origin = config.origin.clone();
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let connectors = Connectors::new(config, store).unwrap();
        let relays = Relays::new(RelayConfig {
            connector: connectors.get("case-law").unwrap(),
            origin,
            day_file: scratch.path().join("connectors").join("day.json"),
            clock: Arc::new(SystemClock),
            local_day: Arc::new(FixedOffset(19_800)),
            events: Arc::new(Quiet),
        });
        let key = SessionKey {
            matter: "01900000-0000-7000-8000-000000000007".parse().unwrap(),
            thread: "01900000-0000-7000-8000-000000000008".parse().unwrap(),
            launch: 1,
        };
        let (session, token) = relays
            .open(key, bind_loopback_exclusive(0).unwrap())
            .unwrap();
        Rig {
            fake,
            _scratch: scratch,
            _connectors: connectors,
            _relays: relays,
            session,
            token,
        }
    }

    /// One connection past its hello, as `clauseo-mcp connector` speaks it.
    struct Agent {
        writer: TcpStream,
        reader: BufReader<TcpStream>,
    }

    impl Agent {
        fn connect(rig: &Rig) -> Agent {
            let writer = TcpStream::connect(rig.session.addr()).unwrap();
            // A safety net against a hung test, not a wait: every read is answered sooner.
            writer
                .set_read_timeout(Some(Duration::from_secs(60)))
                .unwrap();
            let mut agent = Agent {
                reader: BufReader::new(writer.try_clone().unwrap()),
                writer,
            };
            let hello = format!(
                r#"{{"clauseo_relay":1,"token":"{}","connector":"case-law"}}"#,
                rig.token.expose()
            );
            agent.send(&hello);
            assert_eq!(agent.recv_line(), r#"{"clauseo_relay":1,"ok":true}"#);
            agent
        }

        fn send(&mut self, line: &str) {
            self.writer.write_all(line.as_bytes()).unwrap();
            self.writer.write_all(b"\n").unwrap();
        }

        fn list(&mut self, id: u64) {
            self.send(&format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#
            ));
        }

        fn recv_line(&mut self) -> String {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            line.trim_end().to_owned()
        }

        /// The next answer: the reply-too-large error (the reply is over 4 MB).
        fn recv_too_large(&mut self) -> u64 {
            let answer: Value = serde_json::from_str(&self.recv_line()).unwrap();
            assert_eq!(answer["error"]["code"], -32603, "{answer}");
            let message = answer["error"]["message"].as_str().unwrap();
            assert!(message.contains("4 MB"), "{message}");
            answer["id"].as_u64().unwrap()
        }
    }

    /// The answer's text around its array of zeros, for request `id`.
    fn wrapping(id: u64) -> (String, &'static str) {
        let head = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":["#);
        (head, "0]}}")
    }

    /// `0,` repeated to fill [`BODY_BYTES`] with the wrapping of a short id.
    fn zeros() -> Vec<u8> {
        let (head, tail) = wrapping(1);
        b"0,".repeat((BODY_BYTES - head.len() - tail.len()) / 2)
    }

    #[test]
    fn one_large_reply_stays_under_its_bound() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rig = rig(None);
        let mut agent = Agent::connect(&rig);
        let (head, tail) = wrapping(1);
        let mut body = head.into_bytes();
        body.extend_from_slice(&zeros());
        body.extend_from_slice(tail.as_bytes());
        assert!(body.len() <= BODY_BYTES);
        let json_type = [("Content-Type", "application/json")];
        rig.fake
            .answer_next("/mcp/case-law", 200, &json_type, &body);
        drop(body);
        let base = baseline();
        agent.list(1);
        assert_eq!(agent.recv_too_large(), 1);
        let peak = peak_above(base);
        println!(
            "relay_memory: one reply of {BODY_BYTES} bytes, peak live above the baseline: \
             {} MiB (bound {} MiB)",
            mib(peak),
            mib(ONE_REPLY_BOUND),
        );
        assert!(peak < ONE_REPLY_BOUND, "peak {peak} bytes");
    }

    #[test]
    fn sixteen_large_replies_at_once_stay_under_sixteen_bounds() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let upstream = Upstream::start(16);
        let rig = rig(Some(&upstream.origin));
        let mut agents: Vec<Agent> = (0..4).map(|_| Agent::connect(&rig)).collect();
        let base = baseline();
        for (conn, agent) in (0u64..).zip(&mut agents) {
            for call in 0..4 {
                agent.list(10 * conn + call + 1);
            }
        }
        let mut ids = Vec::new();
        for agent in &mut agents {
            for _ in 0..4 {
                ids.push(agent.recv_too_large());
            }
        }
        let peak = peak_above(base);
        ids.sort_unstable();
        let mut want: Vec<u64> = (0..4)
            .flat_map(|conn| (1..=4).map(move |call| 10 * conn + call))
            .collect();
        want.sort_unstable();
        assert_eq!(ids, want);
        println!(
            "relay_memory: 16 replies of {BODY_BYTES} bytes at once, peak live above the \
             baseline: {} MiB (bound {} MiB)",
            mib(peak),
            mib(16 * ONE_REPLY_BOUND),
        );
        assert!(peak < 16 * ONE_REPLY_BOUND, "peak {peak} bytes");
    }

    /// An HTTP upstream that holds every MCP request until `all` have arrived, then answers each
    /// with [`BODY_BYTES`] of `[0,0,…]` for its id, written from one shared buffer.
    struct Upstream {
        origin: String,
    }

    impl Upstream {
        fn start(all: usize) -> Upstream {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let zeros: Arc<[u8]> = zeros().into();
            let arrived = Arc::new((Mutex::new(0usize), Condvar::new()));
            std::thread::spawn(move || {
                for stream in listener.incoming().take(all) {
                    let Ok(stream) = stream else { continue };
                    let zeros = Arc::clone(&zeros);
                    let arrived = Arc::clone(&arrived);
                    std::thread::spawn(move || serve(stream, &zeros, &arrived, all));
                }
            });
            Upstream { origin }
        }
    }

    fn serve(stream: TcpStream, zeros: &[u8], arrived: &(Mutex<usize>, Condvar), all: usize) {
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
            .as_u64()
            .unwrap();
        let (count, changed) = arrived;
        let mut count_now = count.lock().unwrap();
        *count_now += 1;
        changed.notify_all();
        while *count_now < all {
            count_now = changed.wait(count_now).unwrap();
        }
        drop(count_now);
        let (head, tail) = wrapping(id);
        let length = head.len() + zeros.len() + tail.len();
        let start = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\
             Connection: close\r\n\r\n{head}"
        );
        let mut stream = stream;
        let _written = stream
            .write_all(start.as_bytes())
            .and_then(|()| stream.write_all(zeros))
            .and_then(|()| stream.write_all(tail.as_bytes()));
    }
}
