//! Where the probe goes (`offline/probe/service.rs`, the security review and stage D's H1/H2):
//! the service and route the agent's launch recorded from the env it hands the agent, here
//! through a test profile hook giving what desktop's hook gives (its proxy variables for
//! `network.upstream_proxy`, a firm plan's endpoint or provider). Every peer is a listener on
//! 127.0.0.1: no test here leaves the machine.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_profile::PacksWritten;
use clauseo_protocol::{
    AgentId, AgentKind, CommandOutcome, ErrorCode, HostCommand, StopReason, ThreadId, TurnId,
};
use clauseo_sandbox::proxy::Route;
use serde_json::{Map, Value};

use super::offline::{answer, ended};
use super::offline_quiet::{
    CANCELLED, PROMPT, cancel, chunk, finish_turn, going, hold, probes, prompting_held,
    release_one, start, wait, wait_steps, watch,
};
use super::rig::Rig;
use crate::ProfileHook;
use crate::actor::turn::offline::Watch;
use crate::actor::turn::offline::probe::{ReachClass, ReachProbe};
use crate::actor::turn::offline::quiet::QUIET_WAIT;

/// A TLS 1.2 handshake record header, as a server's first answer starts.
const SERVER_HELLO: [u8; 5] = [0x16, 0x03, 0x03, 0x00, 0x00];

/// The `NO_PROXY` desktop's launch gives with a proxy (`setup/network/launch.rs`).
const LOOPBACK: &str = "localhost,127.0.0.1,::1";

/// A profile hook whose launch env is whatever the test put in it last.
#[derive(Clone, Default)]
struct EnvHook(Arc<Mutex<BTreeMap<OsString, OsString>>>);

impl EnvHook {
    fn set(&self, vars: &[(&str, String)]) {
        let mut env = self.0.lock().unwrap();
        env.clear();
        env.extend(
            vars.iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
        );
    }
}

impl ProfileHook for EnvHook {
    fn session_meta(&self, _: &AgentId, _: Option<&PacksWritten>) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _: &AgentId) -> BTreeMap<OsString, OsString> {
        self.0.lock().unwrap().clone()
    }
}

/// The proxy variables desktop's launch hands an agent for `proxy`, with `no_proxy`.
fn proxied(proxy: &str, no_proxy: &str) -> Vec<(&'static str, String)> {
    let mut vars = vec![
        ("HTTPS_PROXY", proxy.to_owned()),
        ("HTTP_PROXY", proxy.to_owned()),
        ("NO_PROXY", no_proxy.to_owned()),
    ];
    if cfg!(not(windows)) {
        vars.push(("https_proxy", proxy.to_owned()));
        vars.push(("http_proxy", proxy.to_owned()));
        vars.push(("no_proxy", no_proxy.to_owned()));
    }
    vars
}

/// A fake firm proxy on 127.0.0.1: answers each of `n` `CONNECT`s 200, then the client hello
/// with a server hello. Its port, and the request lines it got.
fn firm_proxy(n: usize) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        (0..n)
            .map(|_| {
                let (mut stream, _) = listener.accept().unwrap();
                let head = read_head(&mut stream);
                stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
                answer_hello(&mut stream);
                head.lines().next().unwrap_or_default().to_owned()
            })
            .collect()
    });
    (port, handle)
}

/// A fake TLS service on 127.0.0.1 answering one client hello.
fn tls_service() -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        answer_hello(&mut stream);
    });
    (port, handle)
}

fn read_head(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut got = Vec::new();
    let mut byte = [0_u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        got.push(byte[0]);
    }
    String::from_utf8(got).unwrap()
}

/// Reads the client hello's record header, then answers a server hello's.
fn answer_hello(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut header = [0_u8; 5];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(&header[..2], &[0x16, 0x03], "a client hello");
    stream.write_all(&SERVER_HELLO).unwrap();
}

/// A port on 127.0.0.1 nothing listens on.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// A Claude rig whose launch gets `vars` from the hook, with `probe`; its first answer chunk
/// handled, then 30 s of silence: the first quiet probe has answered.
fn quiet_claude(test: &str, vars: &[(&str, String)], probe: ReachProbe) -> (Rig, ThreadId) {
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let (mut rig, thread, _) = start(test, Some(AgentKind::Claude), &lines, probe);
    let hook = EnvHook::default();
    hook.set(vars);
    rig.actor.profile = Box::new(hook);
    let _turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    (rig, thread)
}

fn class(rig: &Rig, thread: ThreadId) -> Option<ReachClass> {
    watch(rig, thread).and_then(Watch::class)
}

/// Cancels `thread`'s live turn as the lawyer would, and ends the rig.
fn stop(mut rig: Rig, thread: ThreadId) {
    let turn = live_turn(&rig, thread);
    cancel(&mut rig, thread, turn);
    rig.finish();
}

/// `thread`'s live turn.
fn live_turn(rig: &Rig, thread: ThreadId) -> TurnId {
    rig.actor.threads[&thread]
        .turn
        .current
        .as_ref()
        .unwrap()
        .turn
}

/// Stage D's H1: a firm proxy set, and a direct route to the service refused: the probe goes
/// through the proxy the launch hands the agent, and gets through.
#[test]
fn the_firm_proxy_the_launch_hands_the_agent_is_where_the_probe_goes() {
    let service = closed_port();
    assert!(
        TcpStream::connect(("127.0.0.1", service)).is_err(),
        "direct is refused"
    );
    let (proxy, seen) = firm_proxy(1);
    let mut vars = proxied(&format!("http://127.0.0.1:{proxy}"), "localhost,::1");
    vars.push(("ANTHROPIC_BASE_URL", format!("https://127.0.0.1:{service}")));
    let (rig, thread) = quiet_claude("target-firm", &vars, ReachProbe::real());
    assert_eq!(
        seen.join().unwrap(),
        [format!("CONNECT 127.0.0.1:{service} HTTP/1.1")]
    );
    assert_eq!(class(&rig, thread), Some(ReachClass::Answered));
    assert!(going(&rig, thread) && watch(&rig, thread).is_some_and(Watch::reached));
    stop(rig, thread);
}

/// The security review's item 2: a firm plan's endpoint (`ANTHROPIC_BASE_URL` from the hook)
/// is the host the probe asks for, through the firm's proxy as desktop's launch gives it for
/// `network.upstream_proxy` alone.
#[test]
fn a_plan_endpoint_is_the_host_the_probe_asks_for() {
    let (proxy, seen) = firm_proxy(1);
    let mut vars = proxied(&format!("http://127.0.0.1:{proxy}"), LOOPBACK);
    vars.push((
        "ANTHROPIC_BASE_URL",
        "https://gateway.firm.example/claude".to_owned(),
    ));
    let (rig, thread) = quiet_claude("target-plan", &vars, ReachProbe::real());
    assert_eq!(
        seen.join().unwrap(),
        ["CONNECT gateway.firm.example:443 HTTP/1.1"]
    );
    assert_eq!(class(&rig, thread), Some(ReachClass::Answered));
    stop(rig, thread);
}

/// The agent's `NO_PROXY` holds the service: the probe goes straight, as the agent would.
#[test]
fn the_agents_no_proxy_holding_the_service_sends_the_probe_straight() {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = proxy.local_addr().unwrap().port();
    let (service, served) = tls_service();
    let mut vars = proxied(&format!("http://127.0.0.1:{proxy_port}"), LOOPBACK);
    vars.push(("ANTHROPIC_BASE_URL", format!("https://127.0.0.1:{service}")));
    let (rig, thread) = quiet_claude("target-no-proxy", &vars, ReachProbe::real());
    served.join().unwrap();
    assert_eq!(class(&rig, thread), Some(ReachClass::Answered));
    proxy.set_nonblocking(true).unwrap();
    assert!(proxy.accept().is_err(), "the proxy saw nothing");
    stop(rig, thread);
}

/// The route is the launch's, read once: a hook that hands a later launch another proxy does
/// not move this launch's probes, and none of them looks a route up.
#[test]
fn the_route_is_resolved_once_per_launch() {
    let (probe, calls) = ReachProbe::fixed(true).recorded();
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let (mut rig, thread, _) = start("target-once", Some(AgentKind::Claude), &lines, probe);
    let hook = EnvHook::default();
    hook.set(&proxied("http://127.0.0.1:3128", LOOPBACK));
    rig.actor.profile = Box::new(hook.clone());
    let _turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    wait(&mut rig, thread, QUIET_WAIT);
    hook.set(&proxied("http://127.0.0.1:8080", LOOPBACK));
    wait(&mut rig, thread, QUIET_WAIT);
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    let firm = Route::Http {
        host: "127.0.0.1".to_owned(),
        port: 3128,
    };
    for services in calls {
        let hosts: Vec<_> = services
            .iter()
            .map(|(url, route)| (url.host_str().unwrap().to_owned(), route.clone()))
            .collect();
        assert_eq!(hosts, [("api.anthropic.com".to_owned(), firm.clone())]);
    }
    stop(rig, thread);
}

/// A cloud provider's login (the hook's `CLAUDE_CODE_USE_*`): never probed, each quiet watch
/// reads `unknown_service` (reached), and the backstop still ends the silent turn at 5:00.
fn a_cloud_login_is_never_probed_and_ends_quiet_at_five_minutes(test: &str, var: &str) {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let (mut rig, thread) = quiet_claude(test, &[(var, "1".to_owned())], probe);
    let turn = live_turn(&rig, thread);
    assert_eq!(probes(&count), 0);
    assert_eq!(class(&rig, thread), Some(ReachClass::UnknownService));
    assert!(going(&rig, thread) && watch(&rig, thread).is_some_and(Watch::reached));
    // 4:59, still going; 5:00, the backstop.
    wait_steps(&mut rig, thread, 8, QUIET_WAIT);
    wait(&mut rig, thread, Duration::from_secs(29));
    assert!(going(&rig, thread));
    wait(&mut rig, thread, Duration::from_secs(1));
    assert_eq!(probes(&count), 0);
    finish_turn(&mut rig, thread);
    let log = rig.log(thread);
    assert_eq!(
        ended(&log, turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentQuiet))
    );
    assert_eq!(answer(&log, turn), "Clause five says");
    rig.finish();
}

#[test]
fn a_bedrock_login_is_never_probed_and_ends_quiet_at_five_minutes() {
    a_cloud_login_is_never_probed_and_ends_quiet_at_five_minutes(
        "target-bedrock",
        "CLAUDE_CODE_USE_BEDROCK",
    );
}

#[test]
fn a_vertex_login_is_never_probed_and_ends_quiet_at_five_minutes() {
    a_cloud_login_is_never_probed_and_ends_quiet_at_five_minutes(
        "target-vertex",
        "CLAUDE_CODE_USE_VERTEX",
    );
}

/// Stage D round 2: the window says offline on a Bedrock launch, which has nothing to probe:
/// its watch counts `unknown_service` a miss, so the turn ends `agent.offline` after the
/// 30 s wait, as before, with no probe sent. (A quiet Bedrock turn, above, still waits for the
/// backstop.)
#[test]
fn a_bedrock_launch_whose_window_goes_offline_ends_offline_after_the_wait() {
    let (probe, count) = ReachProbe::fixed(true).counted();
    let lines = [
        PROMPT.to_owned(),
        chunk("Clause five says"),
        CANCELLED.to_owned(),
    ];
    let (mut rig, thread, _) = start(
        "target-bedrock-window",
        Some(AgentKind::Claude),
        &lines,
        probe,
    );
    let hook = EnvHook::default();
    hook.set(&[("CLAUDE_CODE_USE_BEDROCK", "1".to_owned())]);
    rig.actor.profile = Box::new(hook);
    let turn = prompting_held(&mut rig, thread);
    hold(&mut rig, 1);
    release_one(&mut rig);
    let offline = rig.dispatch(20, HostCommand::SetNetwork { online: false });
    assert_eq!(offline.unwrap(), CommandOutcome::Done);
    wait(&mut rig, thread, QUIET_WAIT);
    assert_eq!(probes(&count), 0);
    finish_turn(&mut rig, thread);
    assert_eq!(
        ended(&rig.log(thread), turn),
        (Some(StopReason::Failed), Some(ErrorCode::AgentOffline))
    );
    rig.finish();
}
