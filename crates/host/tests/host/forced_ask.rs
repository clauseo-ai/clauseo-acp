//! Forced ask through the host (`agent/forced_ask.rs`), with the folder check's answer fixed by
//! `HostConfig::with_folder_support` (test-support only), so it runs on every OS. A Claude agent
//! in a matter on a network drive: `SetMode` takes only `default` and `plan`; a plan exit loses
//! its auto-accept option and an answer naming it is refused; a mode the agent moves to itself is
//! switched back; the row says why. A matter with no such answer runs as before. Codex in such a
//! matter is refused. The `_meta` layers (no `allowedTools`, the hook before every tool with
//! `--ask-all`) are pinned in `agent/loads/tests.rs`, the hook's answers in
//! `pre_write_guard/tests/ask.rs`.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, ErrorCode, FolderSupportKind, HostCommand,
    MatterId, StopReason, StreamId, ThreadId,
};

use crate::codex::{CODEX, run_turn, turn_failed};
use crate::matters::summary;
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// A host whose launches' folder check answers `kind`, with a Claude thread playing `scenario`.
fn claude_in(
    test: &str,
    scenario: &'static str,
    kind: Option<FolderSupportKind>,
) -> (Dir, Running, MatterId, ThreadId) {
    let dir = Dir::new(test, &[(CLAUDE, Scenario::Local(scenario))]);
    let host = dir.start_with(0, |config| config.with_folder_support(kind));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    (dir, host, matter, thread)
}

/// The option ids of the first logged permission request.
fn offered(events: &Events) -> Vec<String> {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(
                asked
                    .options
                    .iter()
                    .map(|option| option.option_id.to_string())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// Every `ModeChanged`'s mode, in order.
fn modes(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ModeChanged(changed) => Some(changed.mode_id.to_string()),
            _ => None,
        })
        .collect()
}

fn set_mode(thread: ThreadId, mode: &str) -> HostCommand {
    HostCommand::SetMode {
        thread,
        mode_id: AgentChoiceId::new(mode).unwrap(),
    }
}

fn answer(thread: ThreadId, events: &Events, option: &str) -> HostCommand {
    HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[0],
        option_id: AgentChoiceId::new(option).unwrap(),
    }
}

#[test]
fn claude_on_a_network_drive_runs_in_forced_ask() {
    let (_dir, host, matter, thread) = claude_in(
        "forced_ask_claude",
        "claude-forced-ask",
        Some(FolderSupportKind::Network),
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Plan the change to clause 4.");
    events.until("permission request 1", |e| {
        !e.permission_requests().is_empty()
    });
    // The plan exit's auto-accept option is not offered, and naming it is refused.
    assert_eq!(offered(&events), ["default", "plan"], "{}", events.dump());
    host.refused(
        answer(thread, &events, "acceptEdits"),
        ErrorCode::PermissionUnknownOption,
    );
    assert_eq!(
        host.ok(answer(thread, &events, "default")),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // The agent moved itself to acceptEdits; the host switched it back before the turn ended.
    // The first `default` is the session's start.
    events.until("ModeChanged default", |e| modes(e).len() == 3);
    assert_eq!(
        modes(&events),
        ["default", "acceptEdits", "default"],
        "{}",
        events.dump()
    );
    // Only the ask modes may be set; the refused ones never reach the agent.
    for mode in ["acceptEdits", "bypassPermissions", "dontAsk", "x"] {
        host.refused(set_mode(thread, mode), ErrorCode::CommandInvalid);
    }
    assert_eq!(host.ok(set_mode(thread, "plan")), CommandOutcome::Done);
    events.until("ModeChanged plan", |e| modes(e).len() == 4);
    events.assert_no_mismatch();
    assert_eq!(
        summary(&host, matter).folder_support,
        Some(FolderSupportKind::Network)
    );
    host.shutdown();
}

/// With no folder answer (and with `cant_change_permissions`, which forces nothing) the same plan
/// exit offers all three options, the auto-accept one is taken, the agent stays in acceptEdits,
/// and `SetMode acceptEdits` reaches it: as before forced ask.
#[test]
fn claude_elsewhere_runs_as_before() {
    for (test, kind) in [
        ("forced_ask_none", None),
        (
            "forced_ask_cant_change",
            Some(FolderSupportKind::CantChangePermissions),
        ),
    ] {
        let (_dir, host, _, thread) = claude_in(test, "claude-plan-exit", kind);
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Plan the change to clause 4.");
        events.until("permission request 1", |e| {
            !e.permission_requests().is_empty()
        });
        assert_eq!(
            offered(&events),
            ["acceptEdits", "default", "plan"],
            "{test}"
        );
        assert_eq!(
            host.ok(answer(thread, &events, "acceptEdits")),
            CommandOutcome::Done
        );
        let ended = events.until_turn_ended(turn);
        assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
        assert_eq!(
            host.ok(set_mode(thread, "acceptEdits")),
            CommandOutcome::Done
        );
        events.until("ModeChanged acceptEdits twice", |e| modes(e).len() == 3);
        events.assert_no_mismatch();
        assert_eq!(
            modes(&events),
            ["default", "acceptEdits", "acceptEdits"],
            "{test}"
        );
        host.shutdown();
    }
}

/// The fixed answer refuses Codex as the Windows check would, on every OS.
#[test]
fn codex_on_a_network_drive_is_refused_on_every_os() {
    let dir = Dir::new(
        "forced_ask_codex",
        &[(CODEX, Scenario::Bundled("codex-handshake"))],
    );
    let host = dir.start_with(0, |config| {
        config.with_folder_support(Some(FolderSupportKind::Network))
    });
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    let failed = turn_failed(&events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(failed.code, ErrorCode::MatterFolderNetwork);
    assert_eq!(
        summary(&host, matter).folder_support,
        Some(FolderSupportKind::Network)
    );
    host.shutdown();
}

/// A folder check that failed is unknown, and unknown fails closed for Claude: forced ask, all
/// four layers. Here `SetMode acceptEdits` is refused and the session's `_meta` carries the hook
/// for every tool with `--ask-all` and no `allowedTools` (the scenario checks the request); the
/// row shows nothing.
#[test]
fn claude_whose_folder_check_failed_runs_in_forced_ask() {
    let dir = Dir::new(
        "forced_ask_unknown",
        &[(CLAUDE, Scenario::Local("claude-ask-unknown"))],
    );
    let host = dir.start_with(0, |config| {
        config
            .with_folder_check_failed()
            .with_pre_write_guard(std::path::PathBuf::from("/opt/clauseo/clauseo-desktop"))
    });
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    host.refused(set_mode(thread, "acceptEdits"), ErrorCode::CommandInvalid);
    events.until("the session attached", |e| {
        e.events()
            .any(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
    });
    events.assert_no_mismatch();
    assert_eq!(summary(&host, matter).folder_support, None);
    host.shutdown();
}

/// A failed check does not refuse Codex: its own sandbox applies.
#[test]
fn codex_whose_folder_check_failed_runs() {
    let dir = Dir::new(
        "forced_ask_codex_unknown",
        &[(CODEX, Scenario::Bundled("codex-handshake"))],
    );
    let host = dir.start_with(0, clauseo_host::HostConfig::with_folder_check_failed);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(summary(&host, matter).folder_support, None);
    host.shutdown();
}
