//! The authorize URL, the loopback listener, the callback checks and the token exchange.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use clauseo_connectors::Clock;
use clauseo_connectors::fake::{FakeOptions, http};
use clauseo_connectors::{CancelFlag, CredentialKind, CredentialStatus, SignInError, SystemClock};
use sha2::Digest as _;
use url::Url;

use crate::support::*;

fn port_of(url: &Url) -> u16 {
    let redirect = url
        .query_pairs()
        .find(|(name, _)| name == "redirect_uri")
        .unwrap()
        .1
        .into_owned();
    Url::parse(&redirect).unwrap().port().unwrap()
}

fn callback(port: u16, query: &str) -> clauseo_connectors::fake::RawResponse {
    let url = Url::parse(&format!("http://127.0.0.1:{port}/oauth/callback?{query}")).unwrap();
    http("GET", &url, &[], &[]).unwrap()
}

fn state_of(url: &Url) -> String {
    url.query_pairs()
        .find(|(name, _)| name == "state")
        .unwrap()
        .1
        .into_owned()
}

#[test]
fn the_authorize_url_matches_the_snapshot() {
    let rig = rig();
    rig.handle.seed_rng_for_test(28);
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let url = pending.authorize_url().as_str().to_owned();
    let port = port_of(pending.authorize_url());
    let base = rig.fake.base().to_owned();
    let snapshot = url
        .replace(
            &url::form_urlencoded::byte_serialize(base.as_bytes()).collect::<String>(),
            "BASE",
        )
        .replace(&base, "BASE")
        .replace(&format!("%3A{port}%2F"), "%3APORT%2F");
    assert_eq!(
        snapshot,
        "BASE/authorize?response_type=code\
         &client_id=BASE%2Foauth%2Fclients%2Fclauseo-desktop.json\
         &redirect_uri=http%3A%2F%2F127.0.0.1%3APORT%2Foauth%2Fcallback\
         &scope=case-law+offline_access\
         &state=STATE\
         &code_challenge=CHALLENGE\
         &code_challenge_method=S256\
         &resource=BASE%2Fmcp%2Fcase-law\
         &resource=BASE%2Fv1%2Fcase-law"
            .replace("STATE", &state_of(pending.authorize_url()))
            .replace(
                "CHALLENGE",
                &pending
                    .authorize_url()
                    .query_pairs()
                    .find(|(name, _)| name == "code_challenge")
                    .unwrap()
                    .1
            )
    );
    // The seeded generator gives the same state and challenge again.
    drop(pending);
    let first_state = state_of(&Url::parse(&url).unwrap());
    rig.handle.seed_rng_for_test(28);
    let again = rig.handle.begin_sign_in(None).unwrap();
    assert_eq!(state_of(again.authorize_url()), first_state);
    assert_eq!(first_state.len(), 22, "16 bytes, base64url");
    let with_hint = rig
        .handle
        .begin_sign_in(Some("advocate@example.in"))
        .unwrap();
    assert!(
        with_hint
            .authorize_url()
            .as_str()
            .ends_with("&login_hint=advocate%40example.in")
    );
}

#[test]
fn the_listener_is_not_reachable_on_the_lan_address() {
    // The machine's LAN address: the source address of a route to a public IP (no packet).
    let lan = UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("192.0.2.1:9").map(|()| socket))
        .and_then(|socket| socket.local_addr())
        .map(|addr| addr.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    let Some(lan) = lan else {
        eprintln!("SKIPPED: this machine has no LAN address");
        return;
    };
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let port = port_of(pending.authorize_url());
    let reached = TcpStream::connect_timeout(&SocketAddr::new(lan, port), Duration::from_secs(1));
    assert!(reached.is_err(), "the listener answered on {lan}");
}

#[test]
fn another_path_gets_404_and_a_wrong_state_400_and_the_listener_keeps_waiting() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let url = pending.authorize_url().clone();
    let port = port_of(&url);
    let finisher = thread::spawn(move || pending.finish());
    let other = Url::parse(&format!("http://127.0.0.1:{port}/favicon.ico")).unwrap();
    assert_eq!(http("GET", &other, &[], &[]).unwrap().status, 404);
    let post = Url::parse(&format!("http://127.0.0.1:{port}/oauth/callback")).unwrap();
    assert_eq!(http("POST", &post, &[], b"code=x").unwrap().status, 404);
    let iss = url::form_urlencoded::byte_serialize(rig.fake.base().as_bytes()).collect::<String>();
    assert_eq!(
        callback(port, &format!("code=x&state=wrong&iss={iss}")).status,
        400
    );
    assert_eq!(callback(port, &format!("code=x&iss={iss}")).status, 400);
    assert_eq!(
        callback(port, "error=access_denied&state=wrong").status,
        400
    );
    // A head over 8 KB gets 431.
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut big = format!("GET /oauth/callback?{} HTTP/1.1\r\n", "a".repeat(9000));
    big.push_str("\r\n");
    let _written = stream.write_all(big.as_bytes());
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer);
    assert!(answer.starts_with("HTTP/1.1 431"), "{answer}");
    assert!(rig.store.peek("case-law").is_none());
    // The real callback still ends the sign-in.
    let page = rig.fake.drive_authorize(&url).unwrap();
    assert_eq!(page.status, 200);
    finisher.join().unwrap().unwrap();
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::SignedIn(CredentialKind::OAuth)
    );
}

#[test]
fn a_wrong_state_never_ends_the_sign_in_before_the_timeout() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.signin_timeout = Duration::from_millis(400);
    });
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let port = port_of(pending.authorize_url());
    let finisher = thread::spawn(move || pending.finish());
    for _ in 0..3 {
        assert_eq!(callback(port, "code=x&state=nope").status, 400);
    }
    assert_eq!(finisher.join().unwrap(), Err(SignInError::TimedOut));
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::NotAdded);
}

#[test]
fn a_missing_or_wrong_iss_fails_the_sign_in_and_stores_nothing() {
    let rig = rig();
    rig.fake.omit_iss();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let (page, result) = complete(&rig.fake, pending);
    check_failure_page(&page);
    assert_eq!(result, Err(SignInError::IssuerMismatch));

    let rig = rig_with(FakeOptions::default(), false, |_| {});
    rig.fake.wrong_iss("http://127.0.0.1:1");
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let (_, result) = complete(&rig.fake, pending);
    assert_eq!(result, Err(SignInError::IssuerMismatch));
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(state_file(&rig), "");
    assert_eq!(rig.fake.requests_to("/token").len(), 0);
}

#[test]
fn access_denied_is_cancelled_and_another_error_is_exchange() {
    let rig = rig();
    rig.fake.deny_authorize(Some("access_denied"));
    let pending = rig.handle.begin_sign_in(None).unwrap();
    assert_eq!(complete(&rig.fake, pending).1, Err(SignInError::Cancelled));
    rig.fake.deny_authorize(Some("server_error"));
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let result = complete(&rig.fake, pending).1;
    assert!(matches!(result, Err(SignInError::Exchange(ref why)) if why.contains("server_error")));
    assert_eq!(rig.fake.requests_to("/token").len(), 0);
}

#[test]
fn the_timeout_ends_finish_and_closes_the_port() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.signin_timeout = Duration::from_millis(200);
    });
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let port = port_of(pending.authorize_url());
    let started = SystemClock.now();
    assert_eq!(pending.finish(), Err(SignInError::TimedOut));
    let took = SystemClock.now().duration_since(started).unwrap();
    assert!(
        took >= Duration::from_millis(200) && took < Duration::from_secs(2),
        "{took:?}"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is still open"
    );
}

/// The listener looks at the flag every 20 ms, so a cancel normally ends `finish` within a tick;
/// the 100 ms target is printed, not asserted. The bound is 2 s because shared CI runners stall
/// (101 to 162 ms were seen on the macOS merge-queue runner); against the 5 min sign-in timeout it
/// still shows the flag acted.
#[test]
fn the_cancel_flag_ends_finish_promptly() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let flag = pending.cancel_flag();
    let finisher = thread::spawn(move || {
        let result = pending.finish();
        (result, SystemClock.now())
    });
    thread::park_timeout(Duration::from_millis(50));
    let cancelled_at = SystemClock.now();
    flag.cancel();
    let (result, ended_at) = finisher.join().unwrap();
    assert_eq!(result, Err(SignInError::Cancelled));
    let took = ended_at.duration_since(cancelled_at).unwrap_or_default();
    eprintln!("cancel to the end of finish: {took:?} (target 100 ms)");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn a_second_begin_cancels_the_first_and_a_dropped_one_closes_its_port() {
    let rig = rig();
    let first = rig.handle.begin_sign_in(None).unwrap();
    let first_port = port_of(first.authorize_url());
    let second = rig.handle.begin_sign_in(None).unwrap();
    assert_eq!(first.finish(), Err(SignInError::Cancelled));
    assert!(TcpStream::connect(("127.0.0.1", first_port)).is_err());
    let second_port = port_of(second.authorize_url());
    assert!(TcpStream::connect(("127.0.0.1", second_port)).is_ok());
    drop(second);
    assert!(TcpStream::connect(("127.0.0.1", second_port)).is_err());
}

#[test]
fn the_success_page_is_fixed_with_no_script_and_no_store() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let (page, result) = complete(&rig.fake, pending);
    result.unwrap();
    // The page goes out only after the exchange and the keychain write.
    assert!(rig.store.peek("case-law").is_some());
    check_page(
        &page,
        200,
        "Signed in. You can close this tab and go back to Clauseo.",
        "साइन इन हो गया",
        15..=40,
    );
}

/// The fixed failure page: 400, the same headers, English then Hindi, about 12 words each.
fn check_failure_page(page: &clauseo_connectors::fake::RawResponse) {
    check_page(
        page,
        400,
        "Sign-in didn't finish. Go back to Clauseo and try again.",
        "साइन इन पूरा नहीं हुआ",
        16..=30,
    );
    let text = page.text();
    let english = text.find("Sign-in didn't finish").unwrap();
    let hindi = text.find("साइन इन पूरा नहीं हुआ").unwrap();
    assert!(english < hindi, "English comes first");
}

/// A fixed page: the status, `text/html; charset=utf-8`, `no-store`, the text, no script and
/// no external resource, and a word count in `words`.
fn check_page(
    page: &clauseo_connectors::fake::RawResponse,
    status: u16,
    english: &str,
    hindi: &str,
    words: std::ops::RangeInclusive<usize>,
) {
    assert_eq!(page.status, status);
    assert_eq!(
        page.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(page.header("cache-control"), Some("no-store"));
    assert_eq!(
        page.header("content-security-policy"),
        Some("default-src 'none'; style-src 'unsafe-inline'")
    );
    let text = page.text();
    assert!(text.contains(english), "{text}");
    assert!(text.contains(hindi), "{text}");
    let lower = text.to_ascii_lowercase();
    for banned in [
        "<script", "src=", "href=", "http:", "https:", "url(", "@import",
    ] {
        assert!(!lower.contains(banned), "{banned}");
    }
    let shown = text
        .split('<')
        .filter_map(|part| part.split_once('>').map(|(_, shown)| shown))
        .filter(|shown| !shown.trim().is_empty() && !shown.contains('{'))
        .collect::<Vec<_>>()
        .join(" ");
    let count = shown.split_whitespace().count();
    assert!(words.contains(&count), "{count} words: {shown}");
}

#[test]
fn the_exchange_sends_the_form_and_stores_the_credential_and_state() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let authorize = pending.authorize_url().clone();
    let (_, result) = complete(&rig.fake, pending);
    result.unwrap();
    let token = rig.fake.requests_to("/token");
    assert_eq!(token.len(), 1);
    let form = token[0].form();
    let names: Vec<&str> = form.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "grant_type",
            "code",
            "redirect_uri",
            "client_id",
            "code_verifier",
            "resource",
            "resource"
        ]
    );
    let value = |name: &str| form.iter().find(|(key, _)| key == name).unwrap().1.clone();
    let query = |name: &str| {
        authorize
            .query_pairs()
            .find(|(key, _)| key == name)
            .unwrap()
            .1
            .into_owned()
    };
    assert_eq!(value("grant_type"), "authorization_code");
    assert!(value("code").starts_with("fake_code_"));
    assert_eq!(value("redirect_uri"), query("redirect_uri"));
    assert_eq!(value("client_id"), query("client_id"));
    let verifier = value("code_verifier");
    assert_eq!(verifier.len(), 43);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, query("code_challenge"));
    let resources: Vec<&String> = form
        .iter()
        .filter(|(key, _)| key == "resource")
        .map(|(_, value)| value)
        .collect();
    let base = rig.fake.base();
    assert_eq!(
        resources,
        [
            &format!("{base}/mcp/case-law"),
            &format!("{base}/v1/case-law")
        ]
    );
    assert!(!form.iter().any(|(key, _)| key == "client_secret"));
    let item = item(&rig).unwrap();
    let keys: Vec<&String> = item.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 5, "{keys:?}");
    assert_eq!(item["v"], 1);
    assert_eq!(item["kind"], "oauth");
    assert!(
        item["access_token"]
            .as_str()
            .unwrap()
            .starts_with("fake_at_")
    );
    assert!(
        item["refresh_token"]
            .as_str()
            .unwrap()
            .starts_with("fake_rt_")
    );
    assert!(item["access_expires_at"].as_u64().unwrap() > 1_700_000_000);
    assert!(rig.store.peek("case-law").unwrap().len() < 300);
    assert_eq!(
        state_file(&rig).trim(),
        r#"{"v":1,"connectors":{"case-law":{"kind":"oauth","ended":null}}}"#
    );
}

#[test]
fn an_expired_code_fails_the_exchange() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let url = pending.authorize_url().clone();
    let port = port_of(&url);
    let finisher = thread::spawn(move || pending.finish());
    // Get a code, let it expire, then deliver the callback by hand.
    let first = http("GET", &url, &[], &[]).unwrap();
    let location = Url::parse(first.header("location").unwrap()).unwrap();
    rig.fake.expire_codes();
    let page = callback(port, location.query().unwrap());
    check_failure_page(&page);
    let result = finisher.join().unwrap();
    assert!(
        matches!(result, Err(SignInError::Exchange(ref why)) if why.contains("invalid_grant")),
        "{result:?}"
    );
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::NotAdded);
}

#[test]
fn a_pkce_mismatch_fails_the_exchange() {
    let rig = rig();
    let mut pending = rig.handle.begin_sign_in(None).unwrap();
    pending.tamper_verifier_for_test();
    let (page, result) = complete(&rig.fake, pending);
    check_failure_page(&page);
    assert!(
        matches!(result, Err(SignInError::Exchange(ref why)) if why.contains("invalid_grant")),
        "{result:?}"
    );
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(state_file(&rig), "");
}

#[test]
fn a_keychain_failure_fails_the_sign_in_and_leaves_the_state_file() {
    let rig = rig();
    rig.store.fail_next_writes(1);
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let (page, result) = complete(&rig.fake, pending);
    check_failure_page(&page);
    assert!(matches!(result, Err(SignInError::Store(_))), "{result:?}");
    assert_eq!(state_file(&rig), "");
    assert_eq!(rig.handle.status(), CredentialStatus::NotAdded);
}

/// Eight connections sending one byte every 500 ms hold every reader; each is cut at its 2 s
/// deadline for the whole head, so the browser's callback, queued behind them, still ends the
/// sign-in within 4 s.
#[test]
fn eight_trickling_connections_do_not_keep_the_callback_out() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let url = pending.authorize_url().clone();
    let port = port_of(&url);
    let finisher = thread::spawn(move || pending.finish());
    let stop = CancelFlag::new();
    let tricklers: Vec<_> = (0..8)
        .map(|_| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let stop = stop.clone();
            thread::spawn(move || {
                for byte in b"GET /oauth/callback?state=x HTTP/1.1\r\n".iter().cycle() {
                    if stream.write_all(&[*byte]).is_err()
                        || stop.wait_timeout(Duration::from_millis(500))
                    {
                        break;
                    }
                }
            })
        })
        .collect();
    // Long enough for the listener to accept all eight and hand each a reader.
    CancelFlag::new().wait_timeout(Duration::from_millis(300));
    let started = SystemClock.now();
    let page = rig.fake.drive_authorize(&url).unwrap();
    let result = finisher.join().unwrap();
    let took = SystemClock.now().duration_since(started).unwrap();
    stop.cancel();
    for trickler in tricklers {
        trickler.join().unwrap();
    }
    assert_eq!(page.status, 200);
    result.unwrap();
    assert!(took < Duration::from_secs(4), "{took:?}");
    assert!(rig.store.peek("case-law").is_some());
}

/// `SignInError` has no `Parse`; a sign-in timeout past the end of the clock is `Exchange`.
#[test]
fn a_sign_in_timeout_that_overflows_the_clock_is_refused() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.signin_timeout = Duration::MAX;
    });
    let pending = rig.handle.begin_sign_in(None).unwrap();
    assert!(matches!(pending.finish(), Err(SignInError::Exchange(_))));
}

/// A flood of 100 connections that send nothing: the readers stay at 8 or fewer and the queue
/// at 64 or fewer (the rest are closed). Once every held connection has timed out, the real
/// callback still ends the sign-in, well within the sign-in timeout.
#[test]
fn a_silent_flood_stays_bounded_and_the_callback_succeeds_after_it() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.signin_timeout = Duration::from_secs(60);
    });
    let pending = rig.handle.begin_sign_in(None).unwrap();
    let url = pending.authorize_url().clone();
    let port = port_of(&url);
    let finisher = thread::spawn(move || pending.finish());
    let flood: Vec<TcpStream> = (0..100)
        .filter_map(|_| TcpStream::connect(("127.0.0.1", port)).ok())
        .collect();
    assert!(flood.len() >= 72, "{} connected", flood.len());
    // Wait until the listener has taken them all and every reader timed out.
    let started = SystemClock.now();
    let drained = loop {
        let load = rig.handle.listener_stats_for_test().unwrap();
        assert!(load.readers <= 8, "{load:?}");
        assert!(load.queued <= 64, "{load:?}");
        if load.peak_readers > 0 && load.readers == 0 && load.queued == 0 {
            break load;
        }
        assert!(
            SystemClock.now().duration_since(started).unwrap() < Duration::from_secs(40),
            "never drained: {load:?}"
        );
        CancelFlag::new().wait_timeout(Duration::from_millis(50));
    };
    assert_eq!(drained.peak_readers, 8, "{drained:?}");
    assert!((16..=64).contains(&drained.peak_queued), "{drained:?}");
    drop(flood);
    let page = rig.fake.drive_authorize(&url).unwrap();
    assert_eq!(page.status, 200);
    finisher.join().unwrap().unwrap();
    assert!(rig.store.peek("case-law").is_some());
}
