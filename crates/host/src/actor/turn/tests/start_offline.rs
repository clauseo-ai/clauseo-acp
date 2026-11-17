//! A start that failed on its connection (`offline/start.rs`): the agent answers `session/new`
//! with an error naming a connection failure, and the turn ends `agent.start_offline` only when
//! the window says offline and the host's probe of the agent's service does not get through
//! either; online, a probe that misses ends it `agent.start_unreachable`. A probe that gets
//! through, an agent whose service is unknown, a text that says a server answered, or a plain
//! internal error ends it `agent.launch_failed`, and the last three probe nothing.

use clauseo_protocol::{AgentKind, CommandOutcome, ErrorCode, HostCommand, StopReason};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::offline::ended;
use super::rig::Rig;
use crate::actor::turn::offline::probe::{Reach, ReachClass, ReachProbe};
use crate::actor::turn::offline::start::start_code;
use crate::pipeline::tests::support::TempDir;

/// Claude Code's line for a refused connection, as claude-agent-sdk 0.3.293 writes it.
pub(super) const REFUSED: &str = "API Error: Unable to connect to API (ECONNREFUSED)";

/// A rig whose agent plays `steps` (scenario lines, no header, no handshake); its kind for the
/// probe `kind`, its probe `probe`.
pub(super) fn start_rig(
    test: &str,
    steps: &str,
    kind: Option<AgentKind>,
    probe: ReachProbe,
) -> Rig {
    let dir = TempDir::new(test);
    let root = dir.path();
    Rig::prepare(root, test, "", None);
    let header =
        format!(r#"{{"scenario":"{test}","description":"a unit test","on_stdin_close":"exit"}}"#);
    let path = root.join(format!("{test}.jsonl"));
    crate::write::write_file(&path, format!("{header}\n{steps}").as_bytes()).unwrap();
    let mut rig = Rig::open(dir, 0, None);
    rig.actor.turn_path.probe = probe;
    rig.actor.turn_path.kind_for_tests = kind;
    rig
}

/// A rig whose agent answers `initialize`, then `session/new` with -32603 and `message`.
pub(super) fn failing_start(
    test: &str,
    message: &str,
    kind: Option<AgentKind>,
    probe: ReachProbe,
) -> Rig {
    let steps = format!(
        r#"{{"expect":{{"method":"initialize","as":"init","params":{{"protocolVersion":1}}}}}}
{{"respond":{{"to":"init","result":{{"protocolVersion":1,"agentCapabilities":{{"loadSession":false}},"authMethods":[],"agentInfo":{{"name":"fake_agent","version":"0.0.0"}}}}}}}}
{{"expect":{{"method":"session/new","as":"new","params":{{"cwd":"{{{{any}}}}"}}}}}}
{{"respond":{{"to":"new","error":{{"code":-32603,"message":"{message}"}}}}}}
"#
    );
    start_rig(test, &steps, kind, probe)
}

/// The window says the computer is offline (`SetNetwork`), as command 9.
fn window_offline(rig: &mut Rig) {
    let answer = rig.dispatch(9, HostCommand::SetNetwork { online: false });
    assert_eq!(answer.unwrap(), CommandOutcome::Done);
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
fn a_connection_error_the_probe_confirms_with_the_window_offline_is_start_offline() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = failing_start("start-offline", REFUSED, Some(AgentKind::Claude), probe);
    window_offline(&mut rig);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentStartOffline)
    );
    assert!(count.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    rig.finish();
}

/// Online, a probe that misses is the service out of reach (a slow network, a firm's proxy),
/// never "You're offline".
#[test]
fn a_connection_error_whose_probe_misses_while_online_is_start_unreachable() {
    let (probe, count) = ReachProbe::fixed(false).counted();
    let mut rig = failing_start("start-unreachable", REFUSED, Some(AgentKind::Claude), probe);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentStartUnreachable)
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    rig.finish();
}

/// The window says offline, but the probe gets through: not offline, a failed start.
#[test]
fn a_connection_error_whose_probe_gets_through_with_the_window_offline_is_a_failed_start() {
    let (probe, _) = ReachProbe::fixed(true).counted();
    let mut rig = failing_start("start-window-only", REFUSED, Some(AgentKind::Claude), probe);
    window_offline(&mut rig);
    assert_eq!(
        first_turn_code(&mut rig),
        Some(ErrorCode::AgentLaunchFailed)
    );
    rig.finish();
}

/// Each side of the offline truth rule (`hold.rs`), as a start reads it.
#[test]
fn a_start_reads_the_probe_by_the_offline_truth_rule() {
    let cases = [
        (ReachClass::Refused, true, ErrorCode::AgentStartOffline),
        (ReachClass::Refused, false, ErrorCode::AgentStartUnreachable),
        (
            ReachClass::ConnectTimeout,
            false,
            ErrorCode::AgentStartUnreachable,
        ),
        (
            ReachClass::TlsSilent,
            false,
            ErrorCode::AgentStartUnreachable,
        ),
        (
            ReachClass::TunnelRefused,
            true,
            ErrorCode::AgentStartOffline,
        ),
        (ReachClass::Answered, true, ErrorCode::AgentLaunchFailed),
        (ReachClass::Answered, false, ErrorCode::AgentLaunchFailed),
        (ReachClass::ProxyAuth, false, ErrorCode::AgentLaunchFailed),
        // Nothing to probe counts as a miss only while the window says offline.
        (
            ReachClass::UnknownService,
            true,
            ErrorCode::AgentStartOffline,
        ),
        (
            ReachClass::UnknownService,
            false,
            ErrorCode::AgentLaunchFailed,
        ),
        // A probe of nothing says nothing.
        (ReachClass::NoService, true, ErrorCode::AgentLaunchFailed),
        (ReachClass::NoService, false, ErrorCode::AgentLaunchFailed),
    ];
    for (class, offline, code) in cases {
        assert_eq!(
            start_code(Reach::of(class), offline),
            code,
            "{class:?} offline={offline}"
        );
    }
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
