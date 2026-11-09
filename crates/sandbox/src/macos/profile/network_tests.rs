//! The agent profile's network: TCP out only, by family, and no inet bind or listener under any
//! combination of the builder's network options (0038: Seatbelt checks a bind and a listen by
//! the local port alone, so no rule could hold a jailed listener to loopback).

use std::path::PathBuf;

use super::SeatbeltProfile;
use crate::macos::no_bind::inet_binds;
use crate::test_support::TempDir;
use crate::{BrowserBridge, Loopback, Policy, PolicyBuilder};

const TAG: &str = "clauseo-4242-8";

struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for name in [
            "matter",
            "data/agent-state/m1/codex",
            "data/agent-state/m1/notes",
            "data/browser-runs/r1/staging",
            "data/browser-runs/r1/sock",
            "data/browsers",
            "data/browser-pins",
            "socks",
            "home",
        ] {
            dir.dir(name);
        }
        World { dir }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn bridge(&self) -> BrowserBridge {
        BrowserBridge {
            socket: self.at("data/browser-runs/r1/sock/guard.sock"),
            staging: self.at("data/browser-runs/r1/staging"),
            bundles: vec![self.at("data/browsers")],
            write_denied: vec![self.at("data/browser-pins")],
            ports: vec![41_002],
        }
    }

    fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("matter"), self.at("data/agent-state/m1/codex"))
            .home(self.at("home"))
            .hidden(self.at("data"))
    }
}

/// The text's rules, each from the `(` that opens it at the start of a line.
fn rules(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with('(') {
            out.push(line.to_owned());
        } else if line.starts_with("  ")
            && let Some(last) = out.last_mut()
        {
            last.push('\n');
            last.push_str(line);
        }
    }
    out
}

/// Every combination of the network options: proxy ports, loopback, the browser socket folder,
/// the bridge, the Keychain and the notes folder. A combination the builder refuses (a bridge
/// with `Loopback::Any` or a socket folder) is left out; the rest are rendered.
fn every_profile(w: &World) -> Vec<String> {
    let mut texts = Vec::new();
    let loopbacks = [
        Loopback::None,
        Loopback::Ports(vec![9_222, 9_223]),
        Loopback::Any,
    ];
    for proxies in [&[][..], &[41_234], &[41_234, 41_235]] {
        for loopback in &loopbacks {
            for (sockets, bridged, keychain, notes) in
                (0..16).map(|bits: u8| (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0))
            {
                let mut b = w.builder().loopback(loopback.clone()).keychain(keychain);
                for port in proxies {
                    b = b.proxy_port(*port);
                }
                if sockets {
                    b = b.browser_socket_path(w.at("socks"));
                }
                if bridged {
                    b = b.browser(w.bridge());
                }
                if notes {
                    b = b.notes(w.at("data/agent-state/m1/notes"));
                }
                let Ok(policy) = b.build() else {
                    assert!(bridged, "only a bridge refuses a combination");
                    continue;
                };
                texts.push(
                    SeatbeltProfile::new(&policy, TAG)
                        .unwrap()
                        .text()
                        .to_owned(),
                );
            }
        }
    }
    texts
}

#[test]
fn no_combination_allows_an_inet_bind_or_listener() {
    let w = World::new("net-nobind");
    let texts = every_profile(&w);
    assert!(texts.len() > 50, "{}", texts.len());
    for text in &texts {
        assert_eq!(inet_binds(&rules(text)), Vec::<String>::new(), "{text}");
        assert!(!text.contains("network-inbound"), "{text}");
        for wide in ["(remote ip", "(local ip", "(remote udp", "(local tcp"] {
            assert!(!text.contains(wide), "{wide}: {text}");
        }
    }
}

/// Proxy ports go out over IPv4 only (the proxy listens on 127.0.0.1); the loopback reach
/// goes out over IPv4 and IPv6, TCP only.
#[test]
fn the_reach_is_tcp_out_by_family() {
    let w = World::new("net-family");
    let policy = w
        .builder()
        .proxy_port(41_234)
        .loopback(Loopback::Ports(vec![9_222]))
        .build()
        .unwrap();
    let text = SeatbeltProfile::new(&policy, TAG)
        .unwrap()
        .text()
        .to_owned();
    let out = rules(&text)
        .into_iter()
        .find(|r| r.starts_with("(allow network-outbound"))
        .unwrap();
    assert_eq!(
        out,
        "(allow network-outbound\n  (remote tcp4 \"localhost:41234\")\n  \
         (remote tcp4 \"localhost:9222\")\n  (remote tcp6 \"localhost:9222\"))"
    );
    let any = w.builder().loopback(Loopback::Any).build().unwrap();
    let text = SeatbeltProfile::new(&any, TAG).unwrap().text().to_owned();
    assert!(text.contains("(remote tcp4 \"localhost:*\")\n  (remote tcp6 \"localhost:*\")"));
}
