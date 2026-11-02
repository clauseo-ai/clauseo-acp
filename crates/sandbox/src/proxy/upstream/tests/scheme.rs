//! The scheme a route is looked up for, the OS cache's cap, and what `Upstream`'s `Debug` shows.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::Scheme::{Http, Https};
use super::super::{CACHE_MAX, Env, Lookup, Route, Upstream};
use super::{FakeClock, env, http, lookup, url};

#[test]
fn plain_http_uses_http_proxy_and_asks_the_os_about_http() {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&asked);
    let both = Env {
        https: Some("http://secure-proxy:8443".to_owned()),
        http: Some("http://plain-proxy:8080".to_owned()),
        no_proxy: Some("internal.test".to_owned()),
    };
    let (lookup, _, _) = lookup(Upstream::System, both, http("os-proxy", 1));
    assert_eq!(
        lookup.route(Https, "a.test", 443),
        http("secure-proxy", 8443)
    );
    assert_eq!(lookup.route(Http, "a.test", 80), http("plain-proxy", 8080));
    assert_eq!(lookup.route(Http, "internal.test", 80), Route::Direct);
    // Only HTTPS_PROXY set: plain http asks the OS, about an http:// URL.
    let os = Lookup::with_parts(
        Upstream::System,
        env(Some("http://secure-proxy:8443"), None),
        move |scheme: super::super::Scheme, host: &str, port: u16| {
            seen.lock().unwrap().push((scheme, host.to_owned(), port));
            http("os-proxy", 1)
        },
        FakeClock::default(),
    )
    .unwrap();
    assert_eq!(os.route(Http, "a.test", 80), http("os-proxy", 1));
    assert_eq!(os.route(Https, "a.test", 443), http("secure-proxy", 8443));
    assert_eq!(
        *asked.lock().unwrap(),
        vec![(Http, "a.test".to_owned(), 80)]
    );
    assert_eq!(
        super::super::upstream_for(
            &Upstream::Fixed(url("http://firm-proxy:3128")),
            &url("http://a.test/")
        )
        .unwrap(),
        http("firm-proxy", 3128)
    );
}

#[test]
fn the_os_cache_holds_at_most_1024_answers_and_drops_the_oldest() {
    let (lookup, calls, clock) = lookup(Upstream::System, Env::default(), http("os-proxy", 1));
    for i in 0..=CACHE_MAX {
        lookup.route(Https, &format!("h{i}.test"), 443);
        clock.advance(Duration::from_millis(1));
    }
    assert_eq!(lookup.cached(), CACHE_MAX);
    assert_eq!(calls.load(Ordering::SeqCst), CACHE_MAX + 1);
    lookup.route(Https, &format!("h{CACHE_MAX}.test"), 443);
    lookup.route(Https, "h1.test", 443);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        CACHE_MAX + 1,
        "the newest are kept"
    );
    lookup.route(Https, "h0.test", 443);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        CACHE_MAX + 2,
        "the oldest was dropped"
    );
    assert_eq!(lookup.cached(), CACHE_MAX);
}

#[test]
fn debug_of_an_upstream_shows_only_scheme_host_and_port() {
    let fixed = Upstream::Fixed(url("http://user:secret@firm-proxy:3128/path?token=x#f"));
    let debug = format!("{fixed:?}");
    assert_eq!(debug, "Fixed(http://firm-proxy:3128)");
    assert_eq!(
        format!("{:?}", Upstream::Fixed(url("http://firm-proxy"))),
        "Fixed(http://firm-proxy:80)"
    );
    assert_eq!(format!("{:?}", Upstream::System), "System");
}
