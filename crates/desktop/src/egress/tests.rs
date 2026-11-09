//! The desktop's egress proxies: a host built as the app builds it offers the browser (the
//! stage B bug: no proxy was passed, so every session showed `NoProxy`), and an agent's
//! restart closes the earlier launch's proxy; one proxy per session (two sessions of one matter
//! get two); `close` of an unknown session does nothing; a proxy that does not start stores
//! nothing; and the quit stops the host first, then every proxy still open.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{EgressProxy, HostConfig, McpServer, ProfileHook, SessionKey};
use clauseo_protocol::{AgentId, DeliveryRequest, HostCommand, MatterId, PromptBlock, ThreadId};
use clauseo_sandbox::ports::ListenerFor;
use clauseo_sandbox::{Allowlist, Denial, DenialSink, ProxyConfig, Upstream};
use serde_json::{Map, Value, json};

use super::{DesktopProxies, with_proxies};
use crate::test_support::{AGENT, MATTERS_DIR, Scratch, block_on, open, stream};

/// A sink that drops every denial.
struct Ignore;

impl DenialSink for Ignore {
    fn denied(&self, _d: Denial) {}
}

/// A proxy config that allows nothing and goes straight out.
fn config() -> ProxyConfig {
    ProxyConfig {
        allow: Allowlist::default(),
        silent: Allowlist::default(),
        block: Arc::default(),
        allowed: Arc::default(),
        upstream: Upstream::Direct,
        denials: Arc::new(Ignore),
        record: None,
        // The host opens a session's proxy only when it has the browser.
        browser: true,
        listener: ListenerFor::Other,
    }
}

/// Session `launch` of thread `thread` in matter 1.
fn key(thread: u64, launch: u64) -> SessionKey {
    let id = |n: u64| format!("01900000-0000-7000-8000-{n:012x}");
    SessionKey {
        matter: id(1).parse::<MatterId>().unwrap(),
        thread: id(thread).parse::<ThreadId>().unwrap(),
        launch,
    }
}

/// Whether something listens on `port` of `127.0.0.1`.
fn listening(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_secs(5)).is_ok()
}

#[test]
fn closing_an_unknown_session_does_nothing() {
    let proxies = DesktopProxies::default();
    proxies.close(key(2, 1));
    assert_eq!(proxies.port_for(key(2, 1)), None);
    proxies.open(key(2, 1), config()).unwrap();
    proxies.close(key(3, 1));
    assert!(proxies.port_for(key(2, 1)).is_some(), "the open one stays");
    proxies.stop_all();
}

/// Two sessions of one matter (two threads, and a thread's next launch) get a proxy each, on
/// their own ports; closing one leaves the others listening.
#[test]
fn each_session_of_a_matter_gets_its_own_proxy() {
    let proxies = DesktopProxies::default();
    let sessions = [key(2, 1), key(3, 2), key(2, 3)];
    for session in sessions {
        proxies.open(session, config()).unwrap();
    }
    assert_eq!(proxies.open_count(), 3);
    let ports: Vec<u16> = sessions
        .iter()
        .map(|session| proxies.port_for(*session).unwrap())
        .collect();
    let mut distinct = ports.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "{ports:?}");
    proxies.close(sessions[0]);
    let mut left = vec![sessions[1], sessions[2]];
    left.sort_unstable();
    assert_eq!(proxies.open_sessions(), left);
    assert_eq!(
        proxies.port_for(sessions[0]),
        None,
        "the closed one is gone"
    );
    assert!(listening(ports[1]) && listening(ports[2]));
    proxies.stop_all();
}

/// The quit's `stop_all` stops every proxy still open: none is left.
#[test]
fn stop_all_stops_every_open_proxy() {
    let proxies = DesktopProxies::default();
    proxies.open(key(2, 1), config()).unwrap();
    proxies.open(key(3, 2), config()).unwrap();
    let ports = [
        proxies.port_for(key(2, 1)).unwrap(),
        proxies.port_for(key(3, 2)).unwrap(),
    ];
    assert!(ports.iter().all(|port| listening(*port)));
    proxies.stop_all();
    assert_eq!(proxies.open_count(), 0);
    assert_eq!(proxies.port_for(key(2, 1)), None);
    // A second stop has nothing to do.
    proxies.stop_all();
}

/// A proxy that does not start (here its upstream is refused) stores nothing.
#[test]
fn a_proxy_that_does_not_start_stores_nothing() {
    let proxies = DesktopProxies::default();
    let refused = ProxyConfig {
        upstream: Upstream::Fixed("http://proxy.invalid:8080/with-a-path".parse().unwrap()),
        ..config()
    };
    assert!(proxies.open(key(2, 1), refused).is_err());
    assert_eq!(proxies.open_count(), 0);
    assert_eq!(proxies.port_for(key(2, 1)), None);
}

/// The quit stops the host first, with the proxies still open, then the proxies.
#[test]
fn the_quit_stops_the_host_then_the_proxies() {
    let proxies = DesktopProxies::default();
    proxies.open(key(2, 1), config()).unwrap();
    let ended = super::stop_in_order(
        || {
            assert_eq!(
                proxies.open_count(),
                1,
                "the proxy is open while the host stops"
            );
            clauseo_host::HostExit::Stopped
        },
        Some(&proxies),
    );
    assert!(matches!(ended, clauseo_host::HostExit::Stopped));
    assert_eq!(proxies.open_count(), 0);
}

/// A profile that offers only a `browser` entry whose sidecars are this test binary, so the
/// launch prep finds them.
struct BrowserOnly;

impl ProfileHook for BrowserOnly {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn mcp_servers(&self, _agent: &AgentId) -> Vec<McpServer> {
        let exe = std::env::current_exe().unwrap();
        vec![McpServer::Stdio {
            name: "browser".to_owned(),
            command: exe.clone(),
            args: vec![
                "browser-guard".to_owned(),
                "--agent-browser".to_owned(),
                exe.display().to_string(),
                "--cwd".to_owned(),
                "{browser_dir}".to_owned(),
            ],
            env: vec![(
                "AGENT_BROWSER_PROXY".to_owned(),
                "http://127.0.0.1:{proxy_port}".to_owned(),
            )],
        }]
    }
}

/// `fake_agent` lines: `initialize`, a `session/new` that must carry exactly one MCP server,
/// the `browser` entry with its proxy filled in, then one prompt answered `Done.`.
fn lines() -> String {
    let modes = json!({"currentModeId": "default", "availableModes": [
        {"id": "default", "name": "Manual"}]});
    [
        json!({"scenario": "agent", "description": "the browser entry goes", "on_stdin_close": "exit"}),
        json!({"expect": {"method": "initialize", "as": "init", "params": {"protocolVersion": 1}}}),
        json!({"respond": {"to": "init", "result": {"protocolVersion": 1,
            "agentCapabilities": {"loadSession": true}, "authMethods": [],
            "agentInfo": {"name": "fake_agent", "version": "0.0.0"}}}}),
        json!({"expect": {"method": "session/new", "as": "new", "params": {"mcpServers": [
            {"name": "browser", "env": [{"name": "AGENT_BROWSER_PROXY", "value": "{{any}}"}]}
        ]}}}),
        json!({"respond": {"to": "new", "result": {"sessionId": "sess-1", "modes": modes}}}),
        json!({"expect": {"method": "session/prompt", "as": "p", "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"update": {"sessionUpdate": "agent_message_chunk", "messageId": "m-1",
            "content": {"type": "text", "text": "Done."}}}}),
        json!({"respond": {"to": "p", "result": {"stopReason": "end_turn"}}}),
    ]
    .map(|line| line.to_string())
    .join("\n")
}

/// A scratch folder whose settings run `fake_agent` for [`AGENT`] with the scenario `lines`
/// (its `agent.jsonl`), with `tools.browser` on.
fn scenario(test: &str, lines: &str) -> Scratch {
    let bin = clauseo_fake_agent::bin_path()
        .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
    let scratch = Scratch::folder(test, &[]);
    let file = scratch.file("agent.jsonl", lines.as_bytes());
    let settings = json!({
        "agents": {AGENT: {
            "command": bin.display().to_string(),
            "args": ["--scenario", file.display().to_string()],
        }},
        "matters": {"default_location": scratch.dir(MATTERS_DIR).display().to_string()},
        "tools": {"browser": true, "web_search": false},
    });
    scratch.dir("data");
    scratch.file("data/settings.json", settings.to_string().as_bytes());
    scratch
}

/// A host on `scratch` with its egress proxy set by `wire`, the `browser` entry from
/// [`BrowserOnly`] and a forced Chrome.
fn wired_host(
    scratch: &Scratch,
    wire: impl FnOnce(HostConfig) -> HostConfig + Send + 'static,
) -> crate::test_support::Running {
    let managed = scratch.path().join("no-managed-mcp.json");
    #[cfg(unix)]
    let sockets = scratch.dir("sockets");
    scratch.start_with(move |config| {
        let config = wire(config)
            .with_profile_hook(Box::new(BrowserOnly))
            .with_browser_executable(Some(PathBuf::from("/chrome")))
            .with_managed_mcp_path(managed);
        #[cfg(unix)]
        let config = config.with_browser_socket_root(sockets);
        config
    })
}

/// Sends a prompt on `thread` and reads `messages` until the agent's `Done.`, which it sends
/// only after a `session/new` with the browser entry.
fn prompt_until_done(
    host: &crate::test_support::Running,
    messages: &crate::test_support::Messages,
    thread: ThreadId,
) {
    host.run(HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Look it up.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    });
    loop {
        let text = messages.next_text();
        assert!(
            !text.contains(r#""stop":"failed""#),
            "the turn failed: {text}"
        );
        if text.contains("Done.") {
            return;
        }
    }
}

/// The bug stage B found: with the proxies wired as `start_host` wires them, a session's
/// `session/new` carries the `browser` entry (it was always `mcpServers: []`, `NoProxy`). Its
/// proxy is open while the session runs, and closed once the host has stopped it.
#[test]
fn a_host_wired_as_the_app_wires_it_offers_the_browser() {
    let scratch = scenario("egress-browser", &lines());
    let proxies = Arc::new(DesktopProxies::default());
    let wired = Arc::clone(&proxies);
    // As `start_host` wires them.
    let host = wired_host(&scratch, move |config| with_proxies(config, &wired));
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, crate::MAIN_WINDOW);
    block_on(bridge.subscribe(crate::MAIN_WINDOW, stream(thread), None)).unwrap();
    prompt_until_done(&host, &messages, thread);
    assert_eq!(proxies.open_count(), 1, "the session's proxy is open");
    drop(host);
    assert_eq!(proxies.open_count(), 0, "closed when the session ended");
}

/// An agent that exits after its turn starts again for the next prompt: the earlier launch's
/// proxy closed when it ended, and the new launch has its own.
#[test]
fn an_agent_restart_closes_the_earlier_proxy() {
    // No `session/load`, so the next launch starts a new session the same scenario expects.
    let lines = format!("{}\n{}", lines(), json!({"exit": {"code": 0}}))
        .replace(r#""loadSession":true"#, r#""loadSession":false"#);
    let scratch = scenario("egress-restart", &lines);
    let proxies = Arc::new(DesktopProxies::default());
    let logged = Arc::new(Logged {
        inner: Arc::clone(&proxies),
        log: Mutex::default(),
    });
    let wired: Arc<dyn EgressProxy> = Arc::clone(&logged) as Arc<dyn EgressProxy>;
    let host = wired_host(&scratch, move |config| config.with_egress_proxy(wired));
    let thread = host.thread();
    let bridge = host.bridge();
    let messages = open(&bridge, crate::MAIN_WINDOW);
    block_on(bridge.subscribe(crate::MAIN_WINDOW, stream(thread), None)).unwrap();
    prompt_until_done(&host, &messages, thread);
    until_closed(&proxies);
    prompt_until_done(&host, &messages, thread);
    until_closed(&proxies);
    drop(host);
    let log = logged.log.lock().unwrap().clone();
    let [
        ("open", first),
        ("close", first_closed),
        ("open", second),
        ("close", second_closed),
    ] = log.as_slice()
    else {
        panic!("not one launch's proxy after the other: {log:?}");
    };
    assert_eq!(first, first_closed);
    assert_eq!(second, second_closed);
    assert_eq!(first.thread, second.thread);
    assert_ne!(first.launch, second.launch, "the restart is a new launch");
}

/// [`DesktopProxies`] with each open and close recorded, in order.
struct Logged {
    inner: Arc<DesktopProxies>,
    log: Mutex<Vec<(&'static str, SessionKey)>>,
}

impl EgressProxy for Logged {
    fn open(&self, session: SessionKey, config: ProxyConfig) -> Result<(), clauseo_sandbox::Error> {
        self.log.lock().unwrap().push(("open", session));
        self.inner.open(session, config)
    }

    fn port_for(&self, session: SessionKey) -> Option<u16> {
        self.inner.port_for(session)
    }

    fn agent_listener(&self, session: SessionKey) -> Option<clauseo_host::AgentListener> {
        self.inner.agent_listener(session)
    }

    fn close(&self, session: SessionKey) {
        self.log.lock().unwrap().push(("close", session));
        self.inner.close(session);
    }
}

/// Waits until no proxy is open: the session's agent exited and its removal closed the proxy.
fn until_closed(proxies: &DesktopProxies) {
    let (_keep, never) = std::sync::mpsc::channel::<()>();
    for _ in 0..600 {
        if proxies.open_count() == 0 {
            return;
        }
        let _timed_out = never.recv_timeout(Duration::from_millis(50));
    }
    panic!("the ended launch's proxy was not closed");
}
