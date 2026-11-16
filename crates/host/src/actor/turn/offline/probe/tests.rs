//! The probe's network side against listeners on 127.0.0.1 only (no test here leaves the
//! machine): a proxy's `CONNECT` answers, the client hello and the first record back, the cap,
//! and a log with no host or proxy text.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_protocol::ThreadId;
use clauseo_sandbox::proxy::Route;
use url::Url;

use super::route::{client_hello, hello_within, probe_any, probe_route};
use super::{Reach, ReachClass, Via};
use crate::clock::{Clock as _, SystemClock};

/// The cap the tests run with: short, so a silent peer costs a second.
const CAP: Duration = Duration::from_secs(1);

/// The service behind the proxy: never resolved, the proxy is asked for it.
const SERVICE: &str = "https://chatgpt.com";

/// A TLS 1.2 handshake record header, as a server's first answer starts.
const SERVER_HELLO: [u8; 5] = [0x16, 0x03, 0x03, 0x00, 0x00];

/// What the fake peer does once it has the client's first bytes.
#[derive(Clone, Copy)]
enum Then {
    /// Answers the `CONNECT` with this head.
    Head(&'static str),
    /// Answers 200, then this to the client hello (`None`: nothing, until the client closes).
    Tunnel(Option<&'static [u8]>),
    /// Says nothing at all until the client closes.
    Silent,
    /// Answers the client hello straight (a direct service) with this (`None`: nothing).
    Tls(Option<&'static [u8]>),
}

/// A fake peer on 127.0.0.1: its port, and the bytes it got (once the client closed).
fn peer(then: Then) -> (u16, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(CAP * 4)).unwrap();
        let mut got = Vec::new();
        match then {
            Then::Head(head) => {
                read_head(&mut stream, &mut got);
                stream.write_all(head.as_bytes()).unwrap();
            }
            Then::Tunnel(answer) => {
                read_head(&mut stream, &mut got);
                stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
                read_record(&mut stream, &mut got);
                if let Some(answer) = answer {
                    stream.write_all(answer).unwrap();
                }
            }
            Then::Tls(answer) => {
                read_record(&mut stream, &mut got);
                if let Some(answer) = answer {
                    stream.write_all(answer).unwrap();
                }
            }
            Then::Silent => {}
        }
        // Until the client closes (or the read times out).
        let mut rest = Vec::new();
        let _closed = stream.read_to_end(&mut rest);
        got.extend(rest);
        got
    });
    (port, handle)
}

fn read_head(stream: &mut TcpStream, got: &mut Vec<u8>) {
    let mut byte = [0_u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        got.extend_from_slice(&byte);
    }
}

/// Reads one TLS record (its 5-byte header and body).
fn read_record(stream: &mut TcpStream, got: &mut Vec<u8>) {
    let mut header = [0_u8; 5];
    stream.read_exact(&mut header).unwrap();
    let len = usize::from(u16::from_be_bytes([header[3], header[4]]));
    let mut body = vec![0_u8; len];
    stream.read_exact(&mut body).unwrap();
    got.extend_from_slice(&header);
    got.extend_from_slice(&body);
}

/// The route through the fake proxy on `port`.
fn proxy(port: u16) -> Route {
    Route::Http {
        host: "127.0.0.1".to_owned(),
        port,
    }
}

fn service() -> Url {
    Url::parse(SERVICE).unwrap()
}

/// Probes `url` by `route` within [`CAP`]: the answer and how long it took.
fn timed(route: Option<Route>, url: &Url) -> (Reach, Duration) {
    let clock = SystemClock;
    let started = clock.now().mono;
    let reach = probe_route(route, url, CAP);
    (reach, clock.now().mono.saturating_sub(started))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn a_connect_answered_502_is_a_refused_tunnel() {
    let (port, fake) = peer(Then::Head(
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n",
    ));
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert_eq!(
        (reach.class, reach.via, reach.status),
        (ReachClass::TunnelRefused, Via::Proxy, 502)
    );
    assert!(!reach.reached());
    let got = fake.join().unwrap();
    let ask = String::from_utf8_lossy(&got);
    assert!(
        ask.starts_with("CONNECT chatgpt.com:443 HTTP/1.1\r\nHost: chatgpt.com:443\r\n\r\n"),
        "{ask}"
    );
    assert!(
        !ask.contains("Proxy-Authorization"),
        "no credentials: {ask}"
    );
}

#[test]
fn a_407_counts_as_reached() {
    let head = "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic\r\n\r\n";
    let (port, fake) = peer(Then::Head(head));
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert_eq!((reach.class, reach.status), (ReachClass::ProxyAuth, 407));
    assert!(reach.reached());
    fake.join().unwrap();
}

#[test]
fn a_connect_answered_200_then_silence_is_tls_silent_within_the_cap() {
    let (port, fake) = peer(Then::Tunnel(None));
    let (reach, took) = timed(Some(proxy(port)), &service());
    assert_eq!((reach.class, reach.status), (ReachClass::TlsSilent, 200));
    assert!(!reach.reached());
    assert!(took < CAP + Duration::from_millis(500), "{took:?}");
    fake.join().unwrap();
}

#[test]
fn a_proxy_that_never_answers_the_connect_is_tunnel_silent() {
    let (port, fake) = peer(Then::Silent);
    let (reach, took) = timed(Some(proxy(port)), &service());
    assert_eq!((reach.class, reach.status), (ReachClass::TunnelSilent, 0));
    assert!(!reach.reached());
    assert!(took < CAP + Duration::from_millis(500), "{took:?}");
    fake.join().unwrap();
}

#[test]
fn garbage_for_a_connect_answer_is_tunnel_silent() {
    let (port, fake) = peer(Then::Head("SSH-2.0-OpenSSH\r\n\r\n"));
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert_eq!(reach.class, ReachClass::TunnelSilent);
    fake.join().unwrap();
}

#[test]
fn a_server_hello_after_200_is_answered_and_the_hello_names_the_service() {
    let (port, fake) = peer(Then::Tunnel(Some(&SERVER_HELLO)));
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert_eq!(
        (reach.class, reach.via, reach.status),
        (ReachClass::Answered, Via::Proxy, 200)
    );
    assert!(reach.reached());
    let got = fake.join().unwrap();
    let end = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let hello = &got[end..];
    assert_eq!(&hello[..2], &[0x16, 0x03], "a TLS handshake record");
    assert!(contains(hello, b"chatgpt.com"), "the SNI names the service");
}

#[test]
fn an_alert_or_eof_after_200_is_tls_other() {
    const ALERT: &[u8] = &[0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x28];
    let (port, fake) = peer(Then::Tunnel(Some(ALERT)));
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert_eq!(reach.class, ReachClass::TlsOther);
    fake.join().unwrap();
    // HTTP text where a record should be: a proxy's own error page.
    let (port, fake) = peer(Then::Tunnel(Some(b"HTTP/1.1 403 Forbidden\r\n\r\n")));
    assert_eq!(
        timed(Some(proxy(port)), &service()).0.class,
        ReachClass::TlsOther
    );
    fake.join().unwrap();
}

#[test]
fn an_eof_after_200_is_tls_other() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let fake = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        read_head(&mut stream, &mut got);
        stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
        read_record(&mut stream, &mut got);
        // Closes without a word.
    });
    assert_eq!(
        timed(Some(proxy(port)), &service()).0.class,
        ReachClass::TlsOther
    );
    fake.join().unwrap();
}

#[test]
fn direct_tls_answered_and_direct_tls_silent() {
    let (port, fake) = peer(Then::Tls(Some(&SERVER_HELLO)));
    let url = Url::parse(&format!("https://127.0.0.1:{port}")).unwrap();
    let (reach, _) = timed(Some(Route::Direct), &url);
    assert_eq!(
        (reach.class, reach.via, reach.status),
        (ReachClass::Answered, Via::Direct, 0)
    );
    let hello = fake.join().unwrap();
    assert_eq!(&hello[..2], &[0x16, 0x03]);

    let (port, fake) = peer(Then::Tls(None));
    let url = Url::parse(&format!("https://127.0.0.1:{port}")).unwrap();
    // No route read: straight to the host.
    let (reach, took) = timed(None, &url);
    assert_eq!(
        (reach.class, reach.via),
        (ReachClass::TlsSilent, Via::Direct)
    );
    assert!(took < CAP + Duration::from_millis(500), "{took:?}");
    fake.join().unwrap();
}

#[test]
fn an_http_service_is_probed_with_tcp_only() {
    let (port, fake) = peer(Then::Silent);
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let (reach, _) = timed(Some(Route::Direct), &url);
    assert_eq!(reach.class, ReachClass::Plain);
    assert!(reach.reached());
    assert!(fake.join().unwrap().is_empty(), "nothing sent");
    // Through a proxy: the proxy's connection only.
    let (port, fake) = peer(Then::Silent);
    let url = Url::parse("http://models.firm.test:8080").unwrap();
    let (reach, _) = timed(Some(proxy(port)), &url);
    assert_eq!((reach.class, reach.via), (ReachClass::Plain, Via::Proxy));
    assert!(fake.join().unwrap().is_empty(), "nothing sent");
}

#[test]
fn a_closed_port_is_refused_and_a_closed_proxy_too() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = Url::parse(&format!("https://127.0.0.1:{port}")).unwrap();
    let (reach, _) = timed(Some(Route::Direct), &url);
    assert!(closed_miss(reach), "{reach:?}");
    let (reach, _) = timed(Some(proxy(port)), &service());
    assert!(closed_miss(reach), "{reach:?}");
    assert_eq!(reach.via, Via::Proxy);
}

/// A closed local port's miss: `refused`, or on Windows `connect_timeout` when the cap is under
/// the ~2 s Windows spends retrying the SYN before it reports the refusal. Both are misses alike;
/// only the log's class differs.
fn closed_miss(reach: Reach) -> bool {
    let refused = if cfg!(windows) {
        matches!(
            reach.class,
            ReachClass::Refused | ReachClass::ConnectTimeout
        )
    } else {
        reach.class == ReachClass::Refused
    };
    refused && !reach.reached()
}

#[test]
fn the_client_hello_names_the_service_and_none_for_an_ip() {
    let hello = client_hello("api.anthropic.com").unwrap();
    assert_eq!(&hello[..2], &[0x16, 0x03]);
    assert!(contains(&hello, b"api.anthropic.com"));
    let hello = client_hello("127.0.0.1").unwrap();
    assert!(!contains(&hello, b"127.0.0.1"));
}

#[test]
fn the_probe_logs_no_host_or_proxy_text() {
    let captured = Captured::default();
    let dispatch = captured.dispatch();
    let (port, fake) = peer(Then::Head("HTTP/1.1 502 Bad Gateway\r\n\r\n"));
    tracing::dispatcher::with_default(&dispatch, || {
        let reach = probe_route(Some(proxy(port)), &service(), CAP);
        super::log(
            ThreadId::new(uuid::Uuid::now_v7()).unwrap(),
            (reach, reach.reached()),
            true,
            "note",
            1,
        );
    });
    fake.join().unwrap();
    let text = captured.text();
    assert!(
        text.contains("the host's probe of the agent's service"),
        "{text}"
    );
    assert!(text.contains("class=\"tunnel_refused\"") || text.contains("class=tunnel_refused"));
    assert!(text.contains("status=502"), "{text}");
    // The lifecycle target: host.log keeps its INFO lines (`log_file.rs`).
    assert!(text.contains(crate::log_file::LIFECYCLE), "{text}");
    for leak in ["127.0.0.1", "chatgpt"] {
        assert!(!text.contains(leak), "{leak} in the log: {text}");
    }
}

#[test]
fn a_login_of_unknown_kind_is_reached_when_either_service_answers() {
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let refused = Url::parse(&format!("https://127.0.0.1:{closed}")).unwrap();
    let (port, fake) = peer(Then::Tls(Some(&SERVER_HELLO)));
    let answers = Url::parse(&format!("https://127.0.0.1:{port}")).unwrap();
    let both = [(refused.clone(), Route::Direct), (answers, Route::Direct)];
    let reach = probe_any(&both, CAP);
    assert_eq!((reach.class, reach.reached()), (ReachClass::Answered, true));
    fake.join().unwrap();
    // Neither: the first one's miss.
    let reach = probe_any(&[(refused, Route::Direct)], CAP);
    assert!(closed_miss(reach), "{reach:?}");
}

#[test]
fn a_blackholed_first_host_never_hides_a_second_that_answers() {
    // The first accepts, takes the hello and never answers; the tries run side by side, each
    // within the one cap, so the second's answer stands and the whole stays within the cap.
    let (silent, hole) = peer(Then::Tls(None));
    let (port, fake) = peer(Then::Tls(Some(&SERVER_HELLO)));
    let both = [
        (
            Url::parse(&format!("https://127.0.0.1:{silent}")).unwrap(),
            Route::Direct,
        ),
        (
            Url::parse(&format!("https://127.0.0.1:{port}")).unwrap(),
            Route::Direct,
        ),
    ];
    let started = SystemClock.now().mono;
    let reach = probe_any(&both, CAP);
    let took = SystemClock.now().mono.saturating_sub(started);
    assert_eq!((reach.class, reach.reached()), (ReachClass::Answered, true));
    assert!(took < CAP + Duration::from_millis(500), "{took:?}");
    fake.join().unwrap();
    hole.join().unwrap();
}

#[test]
fn a_client_hello_that_cannot_be_written_is_tls_other() {
    let (port, fake) = peer(Then::Silent);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(
        hello_within(&mut stream, "chatgpt.com", CAP),
        ReachClass::TlsOther
    );
    drop(stream);
    assert!(fake.join().unwrap().is_empty(), "nothing went out");
}

/// A log capture: every line written, as text (the relay tests' pattern).
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Captured {
    fn dispatch(&self) -> tracing::Dispatch {
        let sink = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || sink.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::Dispatch::new(subscriber)
    }

    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
