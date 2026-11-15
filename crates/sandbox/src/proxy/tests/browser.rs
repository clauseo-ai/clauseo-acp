//! The browser listener is bound only for a launch whose policy gives the browser reach, and
//! takes only its own credential, in both modes.

use std::io::Write as _;
use std::sync::Arc;

use super::harness::{
    Rig, Setup, Sink, auth_line, echo, read_all, read_head, rig, round_trip, status, token_of,
};
use crate::proxy::{Listener, ProxyMode, Verdict};
use crate::test_support::TempDir;
use crate::{Allowlist, Loopback, Policy, ProxyConfig, Upstream, start_proxy};

fn echo_rig(browser: bool) -> Rig {
    echo_rig_in(browser, ProxyMode::Allowlist)
}

fn echo_rig_in(browser: bool, mode: ProxyMode) -> Rig {
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        browser,
        mode,
        ..Setup::default()
    });
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    rig
}

#[test]
fn a_launch_without_the_browser_binds_only_the_agent_listener() {
    let rig = echo_rig(false);
    assert_eq!(rig.handle.browser_port(), None);
    let debug = format!("{:?}", rig.handle);
    assert!(debug.contains("browser_port: None"), "{debug}");
    // The agent listener still serves.
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
}

#[test]
fn a_launch_with_the_browser_binds_both_listeners() {
    let rig = echo_rig(true);
    let browser = rig.handle.browser_port().expect("a browser listener");
    assert_ne!(browser, rig.handle.port());
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
    // Something listens on the browser port.
    drop(rig.browser());
}

#[test]
fn loopback_ports_without_the_browser_bind_one_listener() {
    // Tier B: a relay's loopback port in the policy, and no browser. The config's default
    // binds no browser listener whatever the policy's loopback says.
    let config = ProxyConfig::new(
        Allowlist::default(),
        Allowlist::default(),
        Upstream::Direct,
        Arc::new(Sink::default()),
    );
    assert!(!config.browser);
    let handle = start_proxy(config).unwrap();
    let dir = TempDir::new("proxy-relay-only");
    dir.dir("m");
    dir.dir("s");
    let relay = 61_150;
    let policy = Policy::builder(dir.path().join("m"), dir.path().join("s"))
        .home(dir.path().join("home"))
        .proxy_port(handle.port())
        .loopback(Loopback::Ports(vec![relay]))
        .build()
        .unwrap();
    assert_eq!(policy.loopback(), &Loopback::Ports(vec![relay]));
    assert_eq!(handle.browser_port(), None);
    assert_eq!(policy.proxy_ports(), &[handle.port()]);
}

#[test]
fn a_tier_c_proxy_s_listeners_and_any_browser_listener_stay_out_of_tier_b_s_range() {
    use crate::ports::{ListenerFor, TIER_B_PORTS};
    for (listener, browser) in [
        (ListenerFor::Other, true),
        (ListenerFor::Other, false),
        (ListenerFor::TierB, true),
    ] {
        let mut config = ProxyConfig::new(
            Allowlist::default(),
            Allowlist::default(),
            Upstream::Direct,
            Arc::new(Sink::default()),
        );
        config.browser = browser;
        config.listener = listener;
        let handle = start_proxy(config).unwrap();
        if let Some(port) = handle.browser_port() {
            assert!(!TIER_B_PORTS.contains(&port), "browser {port}");
        }
        if listener == ListenerFor::Other {
            assert!(
                !TIER_B_PORTS.contains(&handle.port()),
                "agent {}",
                handle.port()
            );
        } else if cfg!(windows) {
            assert!(
                TIER_B_PORTS.contains(&handle.port()),
                "agent {}",
                handle.port()
            );
        }
    }
}

/// Sends a CONNECT to `api.anthropic.com:443` on `stream` with `auth` (a header line, or
/// empty); the answer's head.
fn connect_with(mut stream: std::net::TcpStream, auth: &str) -> (std::net::TcpStream, String) {
    let request = format!("CONNECT api.anthropic.com:443 HTTP/1.1\r\n{auth}\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let head = read_head(&mut stream);
    (stream, head)
}

/// A 407 with the challenge, and the connection closed after it.
fn assert_challenged(mut stream: std::net::TcpStream, head: &str, what: &str) {
    assert_eq!(status(head), 407, "{what}: {head}");
    assert!(
        head.contains("Proxy-Authenticate: Basic realm=\"clauseo\"\r\n"),
        "{what}: {head}"
    );
    assert_eq!(read_all(&mut stream), b"", "{what}: closed after the 407");
}

#[test]
fn the_browser_listener_takes_only_its_own_credential_in_both_modes() {
    for mode in [ProxyMode::Allowlist, ProxyMode::Open] {
        let rig = echo_rig_in(true, mode);
        let agent_token = token_of(&rig.handle);
        let wrong = auth_line(&"0".repeat(64));
        for (what, auth) in [
            ("none", String::new()),
            ("wrong", wrong.clone()),
            ("the agent's token", auth_line(&agent_token)),
        ] {
            let (stream, head) = connect_with(rig.browser(), &auth);
            assert_challenged(stream, &head, &format!("{mode:?} browser {what}"));
        }
        let (mut tunnel, head) = connect_with(rig.browser(), &rig.browser_auth());
        assert_eq!(status(&head), 200, "{mode:?}");
        assert_eq!(round_trip(&mut tunnel, b"in"), b"in");
        drop(tunnel);

        // The agent listener as before: its own token only, not the browser's.
        for (what, auth) in [("none", String::new()), ("wrong", wrong)] {
            let (stream, head) = connect_with(rig.agent(), &auth);
            assert_challenged(stream, &head, &format!("{mode:?} agent {what}"));
        }
        let (stream, head) = connect_with(rig.agent(), &rig.browser_auth());
        assert_challenged(stream, &head, &format!("{mode:?} agent with the browser's"));
        assert_eq!(rig.connect("api.anthropic.com:443").1, 200, "{mode:?}");

        assert_eq!(rig.denials(), Vec::new(), "{mode:?}: a 407 sends no denial");
        // A tunnel is recorded when it ends, so the order of the records is not fixed.
        let records = rig.records(8);
        let key = |listener: Listener, verdict: Verdict| format!("{listener:?} {verdict:?}");
        let mut seen: Vec<String> = records.iter().map(|r| key(r.listener, r.verdict)).collect();
        seen.sort();
        let (b, a) = (Listener::Browser, Listener::Agent);
        let auth = Verdict::AuthRequired;
        let mut expected: Vec<String> = [b, b, b, a, a, a]
            .into_iter()
            .map(|listener| key(listener, auth))
            .chain([key(b, Verdict::Tunnel), key(a, Verdict::Tunnel)])
            .collect();
        expected.sort();
        assert_eq!(seen, expected, "{mode:?}");
        assert!(
            records.iter().all(|r| r.host == "api.anthropic.com"),
            "{records:?}"
        );
    }
}

#[test]
fn the_browser_credential_is_its_own_secret_and_none_without_the_listener() {
    assert!(echo_rig(false).handle.browser_credential().is_none());
    let rig = echo_rig(true);
    let credential = rig.handle.browser_credential().expect("a browser listener");
    assert_eq!(credential.user(), "clauseo");
    let password = credential.password().to_owned();
    assert_eq!(password.len(), 64);
    assert!(
        password.bytes().all(|b| b.is_ascii_hexdigit()),
        "{password}"
    );
    let agent = token_of(&rig.handle);
    assert_ne!(password, agent);
    // The same credential every time it is asked.
    assert_eq!(
        rig.handle.browser_credential().unwrap().password(),
        password
    );
    for debug in [format!("{:?}", rig.handle), format!("{credential:?}")] {
        assert!(!debug.contains(&password), "{debug}");
        assert!(!debug.contains(&agent), "{debug}");
    }
    // Nor in the agent's env, which carries only the agent's token.
    for (_, value) in rig.handle.env() {
        let value = value.map(|v| v.to_string_lossy().into_owned());
        assert!(!value.unwrap_or_default().contains(&password));
    }
}
