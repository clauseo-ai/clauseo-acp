//! The agent profile's browser rules: the bridge's allows, its last-rule denies, the installed
//! browsers' deny with or without a bridge, and the builder's refusals.

use std::path::PathBuf;

use super::super::SeatbeltProfile;
use super::INSTALLED_BROWSERS;
use crate::test_support::TempDir;
use crate::{BrowserBridge, Loopback, Policy, PolicyBuilder};

const TAG: &str = "clauseo-4242-9";

struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for name in [
            "matter",
            "data/agent-state/m1/codex",
            "data/browser-runs/r1/staging",
            "data/browser-runs/r1/sock",
            "data/browsers",
            "data/browser-pins",
            "data/agents/codex-acp",
            "app/Contents/Helpers",
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
            bundles: vec![self.at("app/Contents/Helpers"), self.at("data/browsers")],
            write_denied: vec![self.at("data/browser-pins")],
            ports: vec![41_002, 41_003],
        }
    }

    fn builder(&self) -> PolicyBuilder {
        Policy::builder(self.at("matter"), self.at("data/agent-state/m1/codex"))
            .home(self.at("home"))
            .hidden(self.at("data"))
            .runtime(self.at("data/agents/codex-acp"))
            .proxy_port(41_234)
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

#[test]
fn the_bridge_opens_its_socket_and_staging_and_closes_the_rest_last() {
    let w = World::new("bridge-rules");
    let policy = w.builder().browser(w.bridge()).build().unwrap();
    let profile = SeatbeltProfile::new(&policy, TAG).unwrap();
    let text = profile.text();
    assert!(text.contains(
        "(allow network-outbound\n  (remote unix-socket (path-literal (param \"BROWSER_SOCKET\"))))"
    ));
    assert!(text.contains("(allow file-read*\n  (subpath (param \"BROWSER_STAGING\")))"));
    assert!(
        !text.contains("BROWSER_SOCKETS"),
        "the socket folder rule is gone"
    );
    let all = rules(text);
    let last = all
        .iter()
        .position(|r| r.contains("(param \"BROWSER_BUNDLE_0\")"))
        .unwrap();
    for rule in &all[last..] {
        assert!(
            rule.starts_with("(deny"),
            "an allow after the last rules: {rule}"
        );
    }
    let bundles = &all[last];
    assert!(bundles.starts_with("(deny process-exec file-read* file-map-executable file-write*"));
    assert!(bundles.contains("(param \"BROWSER_BUNDLE_1\")"));
    let pins = &all[last + 1];
    assert!(pins.starts_with("(deny file-write*\n  (subpath (param \"BROWSER_WRITE_DENIED_0\"))"));
    let params = profile.params();
    let runs = params.iter().find(|(n, _)| n == "BROWSER_RUNS").unwrap();
    assert!(runs.1.ends_with("data/browser-runs"), "{runs:?}");
    // The installed browsers come last, in /Applications and in the home's Applications.
    let installed = all.last().unwrap();
    assert!(installed.starts_with("(deny process-exec file-map-executable"));
    assert!(installed.contains("(regex #\"^/Applications/("));
    assert!(installed.contains("(subpath (param \"HOME_APPLICATIONS\"))"));
}

#[test]
fn installed_browsers_are_denied_without_a_bridge_too() {
    let w = World::new("bridge-installed");
    let policy = w.builder().build().unwrap();
    let text = SeatbeltProfile::new(&policy, TAG)
        .unwrap()
        .text()
        .to_owned();
    let installed = rules(&text).pop().unwrap();
    assert!(
        installed.starts_with("(deny process-exec file-map-executable"),
        "{installed}"
    );
    assert!(
        !text.contains("BROWSER_"),
        "no bridge rule without a bridge"
    );
    let regex = super::installed_regex();
    assert_eq!(
        regex.matches('|').count(),
        INSTALLED_BROWSERS.len(),
        "{regex}"
    );
    assert!(regex.starts_with("/([gG][oO][oO][gG][lL][eE] [cC][hH][rR][oO][mM][eE]|"));
}

/// The rendezvous allow lives in the browser profile only: the agent's profile, with a bridge
/// or without, never lets Chrome register or look up its port broker, so a Chromium the agent
/// unpacks into the matter dies at start (0038, T9).
#[test]
fn the_agent_profile_never_allows_a_chromium_mach_name() {
    let w = World::new("bridge-mach");
    let with = w.builder().browser(w.bridge()).build().unwrap();
    let without = w.builder().loopback(Loopback::Any).build().unwrap();
    for policy in [&with, &without] {
        let text = SeatbeltProfile::new(policy, TAG).unwrap().text().to_owned();
        for rule in rules(&text).iter().filter(|r| r.starts_with("(allow")) {
            if rule.contains("mach-register") || rule.contains("mach-lookup") {
                for name in ["chromium", "Chromium", "Chrome", "chrome", "Rendezvous"] {
                    assert!(!rule.contains(name), "a Mach allow names {name}: {rule}");
                }
            }
        }
        assert!(
            !text.contains("mach-register"),
            "the agent registers nothing"
        );
    }
}

#[test]
fn the_builder_refuses_what_the_bridge_cannot_live_with() {
    let w = World::new("bridge-refusals");
    w.dir.dir("data/browser-runs/r1/sock/x");
    w.dir.dir("data/other/r1/staging");
    w.dir.dir("data/browsers/chs");
    let refused = |b: PolicyBuilder| b.build().unwrap_err().to_string();
    let e = refused(w.builder().browser(w.bridge()).loopback(Loopback::Any));
    assert!(e.contains("any port"), "{e}");
    let e = refused(
        w.builder()
            .browser(w.bridge())
            .browser_socket_path(w.at("data/browser-runs/r1/sock")),
    );
    assert!(e.contains("socket folder"), "{e}");
    let e = refused(
        w.builder()
            .browser(w.bridge())
            .runtime(w.at("data/browsers/chs")),
    );
    assert!(e.contains("inside a browser bundle"), "{e}");
    let mut bridge = w.bridge();
    bridge.staging = w.at("data/other/r1/staging");
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("browser-runs/<run>/staging"), "{e}");
    let mut bridge = w.bridge();
    bridge.socket = w.at("data/browser-runs/r1/sock/x/guard.sock");
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("browser socket must be"), "{e}");
    let mut bridge = w.bridge();
    bridge.bundles.push(w.at("data"));
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("browser runs folder"), "{e}");
    // The browser's own ports, as loopback or proxy ports: the agent would reach DevTools.
    let e = refused(
        w.builder()
            .browser(w.bridge())
            .loopback(Loopback::Ports(vec![41_002])),
    );
    assert!(e.contains("browser's port 41002"), "{e}");
    let e = refused(w.builder().browser(w.bridge()).proxy_port(41_003));
    assert!(e.contains("browser's port 41003"), "{e}");
    // A bundle or the pins where the agent writes: planted code would run outside the jail.
    w.dir.dir("matter/shell");
    let mut bridge = w.bridge();
    bridge.bundles.push(w.at("matter/shell"));
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("a path the agent writes"), "{e}");
    let mut bridge = w.bridge();
    bridge.write_denied = vec![w.at("data/agent-state/m1/codex/pins")];
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("a path the agent writes"), "{e}");
    let mut bridge = w.bridge();
    bridge.bundles.push(w.at("data/agent-state"));
    let e = refused(w.builder().browser(bridge));
    assert!(e.contains("a path the agent writes"), "{e}");
    // A loopback of listed ports stays possible with a bridge.
    w.builder()
        .browser(w.bridge())
        .loopback(Loopback::Ports(vec![9_222]))
        .build()
        .unwrap();
}
