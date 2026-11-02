use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use url::Url;

use super::Scheme::{Http, Https};
use super::parse::{self, BadProxy, Bypass, NoProxy};
use super::{CACHE_FOR, Env, Lookup, Route, Upstream};
use crate::{Clock, Error};

mod flights;
mod scheme;

/// A clock the test moves by hand.
#[derive(Clone, Default)]
struct FakeClock(Arc<Mutex<Duration>>);

impl FakeClock {
    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
}

fn http(host: &str, port: u16) -> Route {
    Route::Http {
        host: host.to_owned(),
        port,
    }
}

fn env(https: Option<&str>, no_proxy: Option<&str>) -> Env {
    Env {
        https: https.map(str::to_owned),
        http: None,
        no_proxy: no_proxy.map(str::to_owned),
    }
}

/// A lookup whose OS answers `os` and counts its calls.
fn lookup(upstream: Upstream, env: Env, os: Route) -> (Lookup, Arc<AtomicUsize>, FakeClock) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let clock = FakeClock::default();
    let lookup = Lookup::with_parts(
        upstream,
        env,
        move |_: super::Scheme, _: &str, _: u16| {
            counted.fetch_add(1, Ordering::SeqCst);
            os.clone()
        },
        clock.clone(),
    )
    .unwrap();
    (lookup, calls, clock)
}

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn lookup_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<Lookup>();
}

#[test]
fn fixed_wins_over_env_and_os() {
    let (lookup, calls, _) = lookup(
        Upstream::Fixed(url("http://firm-proxy:3128")),
        env(Some("http://env-proxy:8080"), Some("*")),
        http("os-proxy", 1),
    );
    assert_eq!(
        lookup.route(Https, "api.anthropic.com", 443),
        http("firm-proxy", 3128)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn env_wins_over_os() {
    let (lookup, calls, _) = lookup(
        Upstream::System,
        env(Some("http://env-proxy:8080"), None),
        http("os-proxy", 1),
    );
    assert_eq!(
        lookup.route(Https, "api.anthropic.com", 443),
        http("env-proxy", 8080)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn os_answers_without_env() {
    let (lookup, calls, _) = lookup(Upstream::System, Env::default(), http("os-proxy", 1));
    assert_eq!(
        lookup.route(Https, "api.anthropic.com", 443),
        http("os-proxy", 1)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn direct_is_always_direct() {
    let (lookup, calls, _) = lookup(
        Upstream::Direct,
        env(Some("http://env-proxy:8080"), None),
        http("os-proxy", 1),
    );
    assert_eq!(lookup.route(Https, "api.anthropic.com", 443), Route::Direct);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn unusable_env_falls_to_os() {
    for bad in [
        "https://env-proxy:8443",
        "socks5://env-proxy:1080",
        "http://",
    ] {
        let (lookup, _, _) = lookup(Upstream::System, env(Some(bad), None), http("os-proxy", 1));
        assert_eq!(
            lookup.route(Https, "example.test", 443),
            http("os-proxy", 1),
            "{bad}"
        );
    }
}

#[test]
fn fixed_validation() {
    let refused = [
        ("http://user:secret@proxy:3128", BadProxy::Userinfo),
        ("http://user@proxy:3128", BadProxy::Userinfo),
        ("https://proxy:3128", BadProxy::Https),
        ("socks5://proxy:1080", BadProxy::Scheme),
        ("http://proxy:3128/path", BadProxy::Extra),
        ("http://proxy:3128/?q=1", BadProxy::Extra),
        ("http://proxy:3128/#f", BadProxy::Extra),
    ];
    for (text, why) in refused {
        assert_eq!(parse::fixed_proxy(&url(text)), Err(why), "{text}");
        let error = Lookup::new(Upstream::Fixed(url(text))).err().unwrap();
        assert!(matches!(error, Error::ProxyConfig(_)), "{text}: {error}");
        assert!(!error.to_string().contains("secret"), "{error}");
    }
    assert_eq!(
        parse::fixed_proxy(&url("http://proxy")),
        Ok(http("proxy", 80))
    );
    assert_eq!(
        parse::fixed_proxy(&url("http://proxy:3128/")),
        Ok(http("proxy", 3128))
    );
    assert_eq!(
        parse::fixed_proxy(&url("http://[fd00::1]:3128")),
        Ok(http("fd00::1", 3128))
    );
    assert_eq!(
        parse::fixed_proxy(&url("http://10.0.0.1:8080")),
        Ok(http("10.0.0.1", 8080))
    );
}

#[test]
fn env_userinfo_is_dropped() {
    let proxy = parse::env_proxy("http://user:secret@env-proxy:8080").unwrap();
    assert_eq!(proxy.route, http("env-proxy", 8080));
    assert!(proxy.dropped_userinfo);
    let (lookup, _, _) = lookup(
        Upstream::System,
        env(Some("http://user:secret@env-proxy:8080"), None),
        Route::Direct,
    );
    assert_eq!(
        lookup.route(Https, "example.test", 443),
        http("env-proxy", 8080)
    );
}

#[test]
fn env_forms() {
    let plain = parse::env_proxy("env-proxy:8080").unwrap();
    assert_eq!(plain.route, http("env-proxy", 8080));
    assert!(!plain.dropped_userinfo);
    assert_eq!(
        parse::env_proxy("http://env-proxy").unwrap().route,
        http("env-proxy", 80)
    );
    assert_eq!(
        parse::env_proxy(" http://env-proxy:1/ ").unwrap().route,
        http("env-proxy", 1)
    );
    assert_eq!(parse::env_proxy("https://env-proxy"), Err(BadProxy::Https));
    assert_eq!(
        parse::env_proxy("socks5h://env-proxy"),
        Err(BadProxy::Scheme)
    );
}

#[test]
fn no_proxy_matching() {
    let list = NoProxy::parse(
        " internal.test, .corp.test,*.wild.test, only.test:8443, 10.0.0.5 ,[::1]:443,",
    );
    let cases = [
        ("internal.test", 443, true),
        ("a.internal.test", 443, true),
        ("notinternal.test", 443, false),
        ("corp.test", 443, true),
        ("x.y.corp.test", 443, true),
        ("wild.test", 443, true),
        ("a.wild.test", 443, true),
        ("only.test", 8443, true),
        ("only.test", 443, false),
        ("10.0.0.5", 443, true),
        ("10.0.0.50", 443, false),
        ("::1", 443, true),
        ("::1", 80, false),
        ("api.anthropic.com", 443, false),
    ];
    for (host, port, direct) in cases {
        assert_eq!(list.matches(host, port), direct, "{host}:{port}");
    }
    assert!(NoProxy::parse("a.test, *").matches("anything.test", 443));
    assert!(!NoProxy::parse("").matches("anything.test", 443));
}

#[test]
fn no_proxy_applies_only_to_env() {
    let (with_env, _, _) = lookup(
        Upstream::System,
        env(Some("http://env-proxy:8080"), Some("Internal.Test.")),
        http("os-proxy", 1),
    );
    assert_eq!(
        with_env.route(Https, "WWW.internal.test.", 443),
        Route::Direct
    );
    assert_eq!(
        with_env.route(Https, "example.test", 443),
        http("env-proxy", 8080)
    );

    let (os_only, _, _) = lookup(Upstream::System, env(None, Some("*")), http("os-proxy", 1));
    assert_eq!(
        os_only.route(Https, "internal.test", 443),
        http("os-proxy", 1)
    );

    let (fixed, _, _) = lookup(
        Upstream::Fixed(url("http://firm-proxy:3128")),
        env(None, Some("*")),
        Route::Direct,
    );
    assert_eq!(
        fixed.route(Https, "internal.test", 443),
        http("firm-proxy", 3128)
    );
}

#[test]
fn cache_hits_then_expires_after_five_minutes() {
    let (lookup, calls, clock) = lookup(Upstream::System, Env::default(), http("os-proxy", 1));
    assert_eq!(
        lookup.route(Https, "example.test", 443),
        http("os-proxy", 1)
    );
    assert_eq!(
        lookup.route(Https, "EXAMPLE.test.", 443),
        http("os-proxy", 1)
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "same host, normalised: a hit"
    );
    assert_eq!(
        lookup.route(Https, "example.test", 8443),
        http("os-proxy", 1)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2, "another port: a miss");

    clock.advance(CACHE_FOR.checked_sub(Duration::from_secs(1)).unwrap());
    lookup.route(Https, "example.test", 443);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "under 5 minutes: a hit");

    clock.advance(Duration::from_secs(1));
    lookup.route(Https, "example.test", 443);
    assert_eq!(calls.load(Ordering::SeqCst), 3, "at 5 minutes: expired");
    lookup.route(Https, "example.test", 443);
    assert_eq!(calls.load(Ordering::SeqCst), 3, "refreshed");
}

#[test]
fn failed_os_answer_goes_direct() {
    let answer = Err("the PAC script failed".to_owned());
    assert_eq!(super::settle("example.test", 443, answer), Route::Direct);
    assert_eq!(
        super::settle("example.test", 443, Ok(http("p", 1))),
        http("p", 1)
    );
}

#[test]
fn target_urls() {
    assert_eq!(
        super::target_url(Https, "example.test", 443).as_deref(),
        Some("https://example.test/")
    );
    assert_eq!(
        super::target_url(Https, "example.test", 8443).as_deref(),
        Some("https://example.test:8443/")
    );
    assert_eq!(
        super::target_url(Https, "::1", 443).as_deref(),
        Some("https://[::1]/")
    );
    assert_eq!(super::target_url(Https, "bad host/x?y", 443), None);
    assert_eq!(
        super::target_url(Http, "example.test", 80).as_deref(),
        Some("http://example.test/")
    );
}

#[test]
fn upstream_for_direct_and_fixed() {
    let target = url("https://data.clauseo.chat/mcp/case-law?x=1");
    assert_eq!(
        super::upstream_for(&Upstream::Direct, &target).unwrap(),
        Route::Direct
    );
    let fixed = Upstream::Fixed(url("http://firm-proxy:3128"));
    assert_eq!(
        super::upstream_for(&fixed, &target).unwrap(),
        http("firm-proxy", 3128)
    );
    // A firm setting start_proxy refuses is refused here too, never sent direct.
    for bad in [
        "https://firm-proxy:3128",
        "http://user:secret@firm-proxy:3128",
    ] {
        let error = super::upstream_for(&Upstream::Fixed(url(bad)), &target).unwrap_err();
        assert!(matches!(error, Error::ProxyConfig(_)), "{bad}: {error}");
        assert!(!error.to_string().contains("secret"), "{error}");
    }
}

#[test]
fn pac_result_strings() {
    let cases = [
        ("PROXY a:1; SOCKS5 b:2; DIRECT", http("a", 1)),
        ("SOCKS5 b:2; PROXY a:1", http("a", 1)),
        ("SOCKS b:2; DIRECT; PROXY a:1", Route::Direct),
        ("DIRECT", Route::Direct),
        ("", Route::Direct),
        ("SOCKS5 b:2", Route::Direct),
        ("HTTPS secure:443; PROXY a:1", http("a", 1)),
        ("proxy   a ; DIRECT", http("a", 80)),
        ("HTTP [fd00::1]:3128", http("fd00::1", 3128)),
    ];
    for (text, route) in cases {
        assert_eq!(parse::pac_result(text), route, "{text}");
    }
    assert!(parse::is_pac_form("PROXY a:1; DIRECT"));
    assert!(parse::is_pac_form("DIRECT"));
    assert!(!parse::is_pac_form("a:1;b:2"));
    assert!(!parse::is_pac_form("https=a:1"));
}

#[test]
fn winhttp_proxy_strings() {
    let cases = [
        ("proxy:8080", Some(http("proxy", 8080))),
        ("proxy", Some(http("proxy", 80))),
        ("a:1;b:2", Some(http("a", 1))),
        ("a:1 b:2", Some(http("a", 1))),
        ("http=h:1;https=s:2", Some(http("s", 2))),
        ("https=s:2;http=h:1", Some(http("s", 2))),
        ("HTTPS=s:2", Some(http("s", 2))),
        ("http=h:1", None),
        ("http=h:1;ftp=f:3;socks=k:4", None),
        ("socks=k:4;all:5", Some(http("all", 5))),
        ("http://proxy:3128", Some(http("proxy", 3128))),
        ("https=http://s:2", Some(http("s", 2))),
        ("https://tls-proxy:443", None),
        ("", None),
    ];
    for (text, route) in cases {
        assert_eq!(parse::winhttp_proxy(text), route, "{text}");
    }
}

#[test]
fn winhttp_bypass_lists() {
    let list = Bypass::parse("<local>;*.corp.test; 10.*;Exact.Test;*:8443;http://web.test");
    let cases = [
        ("intranet", 443, true),
        ("a.corp.test", 443, true),
        ("corp.test", 443, false),
        ("10.1.2.3", 443, true),
        ("110.1.2.3", 443, false),
        ("exact.test", 443, true),
        ("sub.exact.test", 443, false),
        ("any.test", 8443, true),
        ("any.test", 443, false),
        ("web.test", 443, true),
        ("api.anthropic.com", 443, false),
    ];
    for (host, port, direct) in cases {
        assert_eq!(list.matches(host, port), direct, "{host}:{port}");
    }
    assert!(!Bypass::parse("").matches("intranet", 443));
    assert!(!Bypass::parse("*.corp.test").matches("intranet", 443));
}

#[cfg(target_os = "macos")]
mod macos {
    use super::http;
    use crate::proxy::upstream::{Route, macos, settle};

    const SCRIPT: &str = r#"
        function FindProxyForURL(url, host) {
            if (host == "via-proxy.test") { return "PROXY 10.0.0.1:3128; DIRECT"; }
            if (host == "via-socks.test") { return "SOCKS5 10.0.0.2:1080; PROXY 10.0.0.3:8080"; }
            return "DIRECT";
        }
    "#;

    #[test]
    fn pac_script_routes() {
        let route = macos::script_route(SCRIPT, "https://via-proxy.test/").unwrap();
        assert_eq!(route, http("10.0.0.1", 3128));
        let route = macos::script_route(SCRIPT, "https://via-socks.test/").unwrap();
        assert_eq!(route, http("10.0.0.3", 8080));
        let route = macos::script_route(SCRIPT, "https://elsewhere.test/").unwrap();
        assert_eq!(route, Route::Direct);
    }

    #[test]
    fn throwing_pac_script_goes_direct() {
        let script = r#"function FindProxyForURL(url, host) { throw new Error("broken"); }"#;
        let answer = macos::script_route(script, "https://via-proxy.test/");
        assert!(answer.is_err(), "{answer:?}");
        assert_eq!(settle("via-proxy.test", 443, answer), Route::Direct);
    }
}

#[cfg(windows)]
mod windows {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::http;
    use crate::proxy::upstream::Route;
    use crate::proxy::upstream::windows::{Pac, Session};

    const SCRIPT: &str = r#"function FindProxyForURL(url, host) {
    if (host == "via-proxy.test") { return "PROXY 10.0.0.1:3128; DIRECT"; }
    return "DIRECT";
}
"#;

    /// Serves the PAC file on 127.0.0.1 to every request, until the test process ends.
    fn serve_pac() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buffer[..n]),
                    }
                }
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{SCRIPT}",
                    SCRIPT.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        format!("http://127.0.0.1:{port}/proxy.pac")
    }

    #[test]
    fn pac_url_routes() {
        let pac = serve_pac();
        let session = Session::open().unwrap();
        let route = session
            .proxy_for_url("https://via-proxy.test/", &Pac::Url(&pac))
            .unwrap();
        assert_eq!(route, http("10.0.0.1", 3128));
        let route = session
            .proxy_for_url("https://elsewhere.test/", &Pac::Url(&pac))
            .unwrap();
        assert_eq!(route, Route::Direct);
    }
}
