//! Codex permission cards never make a standing rule or a session one (`agent/codex_options.rs`):
//! a command loses `accept_execpolicy_amendment`, a file change `allow_for_session`, an MCP tool
//! `allow_always` and `allow_session`, an id codex-acp does not have is dropped, and an answer
//! naming any of them is refused as one the agent did not offer; so is a network amendment.
//! Each scenario's `expect_response` is the only answer the agent may get.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, ErrorCode, HostCommand, StopReason, StreamId,
    ThreadId,
};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

const CODEX: &str = "codex-acp";

/// A host whose Codex agent plays `scenario`, and a thread of it whose turn has started.
fn codex_turn(test: &str, scenario: &'static str) -> (Dir, Running, ThreadId, Events) {
    let dir = Dir::new(test, &[(CODEX, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let events = host.events(StreamId::Thread(thread));
    (dir, host, thread, events)
}

/// The option ids of logged request `n` (0-based), once it is logged.
fn offered(events: &mut Events, n: usize) -> Vec<String> {
    events.until(&format!("permission request {}", n + 1), |e| {
        e.permission_requests().len() > n
    });
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(
                asked
                    .options
                    .iter()
                    .map(|option| option.option_id.to_string())
                    .collect(),
            ),
            _ => None,
        })
        .nth(n)
        .unwrap_or_default()
}

fn answer(thread: ThreadId, events: &Events, n: usize, option: &str) -> HostCommand {
    HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[n],
        option_id: AgentChoiceId::new(option).unwrap(),
    }
}

/// The file change and the command offer allow once and cancel only; the session option, the
/// prefix rule's and the unknown one are refused when named.
#[test]
fn a_codex_command_card_never_offers_the_prefix_rule() {
    let (_dir, host, thread, mut events) =
        codex_turn("codex_no_execpolicy", "codex-no-execpolicy-amendment");
    let (turn, _) = host.prompt(thread, "Mark it done.");
    assert_eq!(
        offered(&mut events, 0),
        ["allow_once", "cancel"],
        "{}",
        events.dump()
    );
    host.refused(
        answer(thread, &events, 0, "allow_for_session"),
        ErrorCode::PermissionUnknownOption,
    );
    assert_eq!(
        host.ok(answer(thread, &events, 0, "allow_once")),
        CommandOutcome::Done
    );
    assert_eq!(
        offered(&mut events, 1),
        ["allow_once", "cancel"],
        "{}",
        events.dump()
    );
    for option in ["accept_execpolicy_amendment", "future_always"] {
        host.refused(
            answer(thread, &events, 1, option),
            ErrorCode::PermissionUnknownOption,
        );
    }
    assert_eq!(
        host.ok(answer(thread, &events, 1, "allow_once")),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}

/// A command offering network amendments reaches the card without them, each refused when
/// named, and the agent gets the lawyer's answer, never an amendment.
#[test]
fn a_codex_network_amendment_never_reaches_the_card_or_the_agent() {
    let (_dir, host, thread, mut events) =
        codex_turn("codex_no_network", "codex-no-network-amendment");
    let (turn, _) = host.prompt(thread, "Fetch it.");
    assert_eq!(
        offered(&mut events, 0),
        ["allow_once", "decline", "cancel"],
        "{}",
        events.dump()
    );
    for option in [
        "apply_network_policy_amendment:0",
        "apply_network_policy_amendment:1",
    ] {
        host.refused(
            answer(thread, &events, 0, option),
            ErrorCode::PermissionUnknownOption,
        );
    }
    assert_eq!(
        host.ok(answer(thread, &events, 0, "decline")),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}

/// The MCP card loses `allow_session` and `allow_always`, each refused when named.
#[test]
fn a_codex_mcp_card_never_offers_always_allow() {
    let (_dir, host, thread, mut events) = codex_turn("codex_no_mcp_always", "codex-no-mcp-always");
    let (turn, _) = host.prompt(thread, "Search the docs.");
    assert_eq!(
        offered(&mut events, 0),
        ["allow_once", "cancel"],
        "{}",
        events.dump()
    );
    for option in ["allow_session", "allow_always"] {
        host.refused(
            answer(thread, &events, 0, option),
            ErrorCode::PermissionUnknownOption,
        );
    }
    assert_eq!(
        host.ok(answer(thread, &events, 0, "allow_once")),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    host.shutdown();
}

/// The live row, run once by hand (`--run-ignored only`, with `TMPDIR=/tmp/codex-always`):
/// the host's real launch of codex-acp 2.2.2 over Codex 0.162.1, through
/// `/tmp/codex-always/codex-acp.sh`, which points Codex at the local mock Responses provider
/// `/tmp/codex-always/mock.mjs` (no login) and runs the shipped adapter. The agent's home holds
/// the prefix rule an earlier "Always" left; the launch removes it, and a `touch` outside the
/// matter, which the rule would have let run unasked, asks with allow once and cancel only.
#[test]
#[ignore = "live: real codex-acp and Codex against /tmp/codex-always's mock provider"]
fn live_a_planted_rule_is_removed_and_the_command_asks_once_or_deny() {
    let dir = Dir::new("codex_live_rules", &[]);
    let settings = serde_json::json!({"agents": {CODEX: {
        "command": "/bin/sh",
        "args": ["/tmp/codex-always/codex-acp.sh"],
        "env": {"CODEX_CONFIG": "{\"model\":\"mock-model\"}"},
    }}});
    crate::write::write(
        &dir.data().join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap().as_bytes(),
    );
    let home = dir.data().join("agents").join(CODEX).join("home");
    let rules = home.join("rules");
    crate::write::create_dir_all(&rules);
    crate::write::write(
        &rules.join("default.rules"),
        b"prefix_rule(pattern=[\"touch\"], decision=\"allow\")\n",
    );
    let marker = std::path::Path::new("/tmp/codex-always/outside/marker.txt");
    if marker.exists() {
        crate::write::remove_file(marker);
    }
    eprintln!("live: planted {}", rules.join("default.rules").display());
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    eprintln!("live: matter folder {}", folder.display());
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Mark the task done.");
    let options = offered(&mut events, 0);
    eprintln!("live: card options {options:?}");
    eprintln!(
        "live: rules after launch: {}",
        if std::fs::symlink_metadata(&rules).is_ok() {
            "present"
        } else {
            "absent"
        }
    );
    eprintln!(
        "live: home at codex-acp start:\n{}",
        std::fs::read_to_string("/tmp/codex-always/home-at-start.txt").unwrap_or_default()
    );
    assert_eq!(options, ["allow_once", "cancel"], "{}", events.dump());
    assert!(std::fs::symlink_metadata(&rules).is_err());
    host.refused(
        answer(thread, &events, 0, "accept_execpolicy_amendment"),
        ErrorCode::PermissionUnknownOption,
    );
    assert_eq!(
        host.ok(answer(thread, &events, 0, "allow_once")),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    eprintln!(
        "live: turn ended {:?}; marker {}; rules {}",
        ended.stop,
        if marker.exists() { "made" } else { "not made" },
        if std::fs::symlink_metadata(&rules).is_ok() {
            "present"
        } else {
            "absent"
        }
    );
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(std::fs::symlink_metadata(&rules).is_err());
    host.shutdown();
}
