//! Open mode with no block list loaded: a proxy with the browser listener does not start
//! (`Error::BlockListNotLoaded`), so the browser never gets a way out that blocks nothing, and
//! a list that loads later lets the next start through. Allowlist mode, and open mode without
//! the browser listener (whose agent listener keeps its allowlist), start as before.

use std::sync::Arc;

use clauseo_blocklist::Current;

use super::blocked::{current, list};
use crate::{Allowlist, Denial, DenialSink, Error, ProxyConfig, ProxyMode, Upstream, start_proxy};

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

fn refused(config: ProxyConfig) -> bool {
    matches!(start_proxy(config), Err(Error::BlockListNotLoaded))
}

#[test]
fn open_mode_with_the_browser_does_not_start_without_a_list() {
    let none = Arc::new(Current::default());
    assert!(refused(config(ProxyMode::Open, true, &none)));
    let message = Error::BlockListNotLoaded.to_string();
    assert!(message.contains("block list"), "{message}");
}

#[test]
fn open_mode_with_the_browser_starts_with_a_list() {
    let handle = start_proxy(config(ProxyMode::Open, true, &current())).unwrap();
    assert!(handle.browser_port().is_some());
}

#[test]
fn a_list_that_loads_later_lets_the_next_start_through() {
    let block = Arc::new(Current::default());
    assert!(refused(config(ProxyMode::Open, true, &block)));
    assert!(block.offer(list("2026-11-07T06:00:00Z", &["evil.example p"]), None));
    let handle = start_proxy(config(ProxyMode::Open, true, &block)).unwrap();
    assert!(handle.browser_port().is_some());
}

#[test]
fn without_the_open_browser_listener_an_unloaded_list_still_starts() {
    let none = Arc::new(Current::default());
    for (mode, browser) in [
        (ProxyMode::Allowlist, true),
        (ProxyMode::Allowlist, false),
        (ProxyMode::Open, false),
    ] {
        let started = start_proxy(config(mode, browser, &none));
        assert!(started.is_ok(), "{mode:?} browser {browser}: {started:?}");
    }
}
