//! Open mode with no block list loaded: a proxy with the browser listener starts (with one
//! warning), lets any public site through, and still refuses private addresses. A list that
//! loads later is enforced from the next request, on the proxy already running.

use std::io::Write as _;
use std::sync::Arc;

use clauseo_blocklist::{Current, Tag};

use super::blocked::{blocked_at, list};
use super::harness::{Rig, Setup, echo, host_denial, read_all, read_head, rig, status};
use crate::HostRefusal::PrivateAddress;
use crate::{Allowlist, Denial, DenialSink, ProxyConfig, ProxyMode, Upstream, start_proxy};

const MADE: &str = "2026-11-07T06:00:00Z";

struct Ignore;

impl DenialSink for Ignore {
    fn denied(&self, _d: Denial) {}
}

fn config(mode: ProxyMode, browser: bool, block: &Arc<Current>) -> ProxyConfig {
    let sink: Arc<dyn DenialSink> = Arc::new(Ignore);
    let empty = Allowlist::default;
    ProxyConfig {
        mode,
        browser,
        block: Arc::clone(block),
        ..ProxyConfig::new(empty(), empty(), Upstream::Direct, sink)
    }
}

/// An open-mode rig with the browser listener and `block`, unloaded so far, where
/// evil.example and public.example resolve to 192.0.2.10 and an echo server answers on 443.
fn open_rig(block: &Arc<Current>) -> Rig {
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        block: Arc::clone(block),
        ..Setup::default()
    });
    rig.net.name("evil.example", &["192.0.2.10"]);
    rig.net.name("public.example", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    rig
}

/// A CONNECT to `target` on the browser listener; the answer's status.
fn browser_status(rig: &Rig, target: &str) -> u16 {
    let mut stream = rig.browser();
    let line = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    stream
        .write_all(format!("{line}{}\r\n", rig.browser_auth()).as_bytes())
        .unwrap();
    let code = status(&read_head(&mut stream));
    if code != 200 {
        drop(read_all(&mut stream));
    }
    code
}

#[test]
fn open_mode_with_the_browser_starts_without_a_list() {
    let none = Arc::new(Current::default());
    let handle = start_proxy(config(ProxyMode::Open, true, &none)).unwrap();
    assert!(handle.browser_port().is_some());
    assert!(none.get().is_unloaded(), "starting loads nothing");
}

#[test]
fn without_a_list_any_public_site_passes_and_a_private_address_does_not() {
    let block = Arc::new(Current::default());
    let rig = open_rig(&block);
    assert_eq!(browser_status(&rig, "evil.example:443"), 200);
    assert_eq!(browser_status(&rig, "public.example:443"), 200);
    rig.net.name("inside.test", &["10.1.2.3"]);
    assert_eq!(browser_status(&rig, "inside.test:443"), 403);
    assert_eq!(browser_status(&rig, "127.0.0.1:443"), 403);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("inside.test", 443, PrivateAddress, false),
            host_denial("127.0.0.1", 443, PrivateAddress, false),
        ]
    );
}

#[test]
fn a_list_that_loads_later_is_enforced_on_the_running_proxy() {
    let block = Arc::new(Current::default());
    let rig = open_rig(&block);
    assert_eq!(browser_status(&rig, "evil.example:443"), 200);
    assert!(block.offer(list(MADE, &["evil.example p"]), None));
    assert_eq!(browser_status(&rig, "evil.example:443"), 403);
    assert_eq!(browser_status(&rig, "public.example:443"), 200);
    assert_eq!(
        rig.denials(),
        vec![blocked_at(
            "evil.example",
            443,
            "evil.example",
            Tag::Phishing,
            MADE
        )]
    );
}

#[test]
fn a_new_proxy_over_a_loaded_list_starts_and_blocks() {
    let block = Arc::new(Current::default());
    assert!(block.offer(list(MADE, &["evil.example p"]), None));
    let rig = open_rig(&block);
    assert_eq!(browser_status(&rig, "evil.example:443"), 403);
}

#[test]
fn every_mode_starts_with_an_unloaded_list() {
    let none = Arc::new(Current::default());
    for (mode, browser) in [
        (ProxyMode::Open, true),
        (ProxyMode::Open, false),
        (ProxyMode::Allowlist, true),
        (ProxyMode::Allowlist, false),
    ] {
        let started = start_proxy(config(mode, browser, &none));
        assert!(started.is_ok(), "{mode:?} browser {browser}: {started:?}");
    }
}
