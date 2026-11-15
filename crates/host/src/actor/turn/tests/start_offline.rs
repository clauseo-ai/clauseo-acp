//! A start that failed on its connection (`offline/start.rs`): the agent answers `session/new`
//! with an error naming a connection failure, and the turn ends `agent.start_offline` only when
//! the host's probe of the agent's service does not get through either. A probe that gets
//! through, an agent whose service is unknown, a text that says a server answered, or a plain
//! internal error ends it `agent.launch_failed`, and the last three probe nothing.

use clauseo_protocol::{AgentKind, ErrorCode, StopReason};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use crate::actor::turn::offline::probe::ReachProbe;
use crate::pipeline::tests::support::TempDir;

/// Claude Code's line for a refused connection, as claude-agent-sdk 0.3.293 writes it.
const REFUSED: &str = "API Error: Unable to connect to API (ECONNREFUSED)";

/// A rig whose agent answers `initialize`, then `session/new` with -32603 and `message`; its
/// kind for the probe `kind`, its probe `probe`.
fn failing_start(test: &str, message: &str, kind: Option<AgentKind>, probe: ReachProbe) -> Rig {
    let dir = TempDir::new(test);
    let root = dir.path();
    Rig::prepare(root, test, "", None);
    let scenario = format!(
        r#"{{"scenario":"{test}","description":"a unit test","on_stdin_close":"exit"}}
{{"expect":{{"method":"initialize","as":"init","params":{{"protocolVersion":1}}}}}}
{{"respond":{{"to":"init","result":{{"protocolVersion":1,"agentCapabilities":{{"loadSession":false}},"authMethods":[],"agentInfo":{{"name":"fake_agent","version":"0.0.0"}}}}}}}}
{{"expect":{{"method":"session/new","as":"new","params":{{"cwd":"{{{{any}}}}"}}}}}}
{{"respond":{{"to":"new","error":{{"code":-32603,"message":"{message}"}}}}}}
"#
    );
    let path = root.join(format!("{test}.jsonl"));
    crate::write::write_file(&path, scenario.as_bytes()).unwrap();
    let mut rig = Rig::open(dir, 0, None);
    rig.actor.turn_path.probe = probe;
    rig.actor.turn_path.kind_for_tests = kind;
    rig
}

/// The code the first turn of `rig` fails with.
fn first_turn_code(rig: &mut Rig) -> Option<ErrorCode> {
    let thread = rig.thread();
    let turn = send_prompt(rig, 10, thread);
    until_ended(rig, thread);
    let (stop, code) = ended(&rig.log(thread), turn);
    assert_eq!(stop, Some(StopReason::Failed));
    assert!(rig.actor.turn_path.start_probes.is_empty());
    code
}

#[test]
fn a_connection_error_the_probe_confirms_is_start_offline() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = failing_start("start-offline", REFUSED, Some(AgentKind::Claude), probe);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentStartOffline)
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    rig.finish();
}

/// The lawyer is online: a connection-sounding error never says offline.
#[test]
fn a_connection_error_whose_probe_gets_through_is_a_failed_start() {
    let (probe, count) = ReachProbe::fixed(true).counted();
    let mut rig = failing_start("start-online", REFUSED, Some(AgentKind::Claude), probe);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentLaunchFailed)
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    rig.finish();
}

#[test]
fn an_agent_of_no_known_service_is_never_probed() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = failing_start("start-no-service", REFUSED, None, probe);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentLaunchFailed)
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    rig.finish();
}

#[test]
fn a_server_answer_or_a_plain_internal_error_is_never_probed() {
    for (test, message) in [
        (
            "start-server",
            "unexpected status 503 Service Unavailable: upstream connect error",
        ),
        ("start-internal", "Internal error"),
    ] {
        let (probe, count) = ReachProbe::fixed(false).counted();
        let mut rig = failing_start(test, message, Some(AgentKind::Claude), probe);
        assert_eq!(
            first_turn_code(&mut rig),
            Some(ErrorCode::AgentLaunchFailed),
            "{test}"
        );
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0, "{test}");
        rig.finish();
    }
}
