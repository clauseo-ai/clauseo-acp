//! Permission requests: answers, an undo refused while a turn runs, the lawyer-edits flag.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand,
    MatterPath, PermissionChoice, StopReason, StreamId, ThreadId,
};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread};
use crate::turn::change_label;
use crate::write;

/// The agent id mapped to `permission-kinds`.
pub(crate) const PERM: &str = "perm-agent";

/// The answers `permission-kinds` expects, in order.
pub(crate) const KINDS_ANSWERS: [&str; 4] =
    ["allow-once", "allow-always", "reject-once", "reject-always"];

/// A dir whose `perm-agent` plays `permission-kinds`.
fn kinds_dir(test: &str) -> Dir {
    Dir::new(test, &[(PERM, Scenario::Bundled("permission-kinds"))])
}

/// Answers the requests of `thread` from the `first`-th on (0-based) with `options`, waiting
/// for each to be requested.
pub(crate) fn answer_from(
    host: &Running,
    events: &mut Events,
    thread: ThreadId,
    first: usize,
    options: &[&str],
) {
    for (n, option) in options.iter().enumerate() {
        let index = first + n;
        events.until(&format!("permission request {}", index + 1), |e| {
            e.permission_requests().len() > index
        });
        let request = events.permission_requests()[index];
        let outcome = host.ok(HostCommand::AnswerPermission {
            thread,
            request,
            option_id: AgentChoiceId::new(option).unwrap(),
        });
        assert_eq!(outcome, CommandOutcome::Done);
    }
}

/// Waits for `perm-1` of `thread`.
pub(crate) fn until_first_request(events: &mut Events) {
    events.until("permission request 1", |e| {
        !e.permission_requests().is_empty()
    });
}

#[test]
fn lawyer_edit_during_turn_is_flagged() {
    let dir = kinds_dir("lawyer_edit_during_turn_is_flagged");
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    write::write(&folder.join("lawyer-notes.txt"), b"Call the client.\n");
    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);

    let changed = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let files: Vec<(&str, &str, bool)> = changed
        .files
        .iter()
        .map(|f| {
            (
                f.path().as_str(),
                change_label(f),
                f.may_include_lawyer_edits(),
            )
        })
        .collect();
    assert_eq!(files, [("lawyer-notes.txt", "added", true)]);
    host.shutdown();
}

#[test]
fn undo_refused_while_turn_runs() {
    let dir = kinds_dir("undo_refused_while_turn_runs");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);

    host.refused(
        HostCommand::UndoStep {
            thread,
            turn,
            files: vec![FileDecision {
                path: MatterPath::new("brief.txt").unwrap(),
                choice: FileChoice::Undo,
            }],
        },
        ErrorCode::MatterTurnRunning,
    );

    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    events.until_turn_ended(turn);
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn permission_first_answer_wins() {
    let dir = kinds_dir("permission_first_answer_wins");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    let request = events.permission_requests()[0];
    let answer = |option: &str| HostCommand::AnswerPermission {
        thread,
        request,
        option_id: AgentChoiceId::new(option).unwrap(),
    };

    host.refused(answer("not-offered"), ErrorCode::PermissionUnknownOption);
    assert_eq!(host.ok(answer("allow-once")), CommandOutcome::Done);
    host.refused(answer("allow-once"), ErrorCode::PermissionNotPending);
    host.refused(answer("reject-once"), ErrorCode::PermissionNotPending);

    answer_from(&host, &mut events, thread, 1, &KINDS_ANSWERS[1..]);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    let resolved = events
        .count(|event| matches!(event, DomainEvent::PermissionResolved(r) if r.request == request));
    assert_eq!(resolved, 1, "{}", events.dump());
    let first = events.events().find_map(|event| match event {
        DomainEvent::PermissionResolved(r) if r.request == request => Some(r.choice.clone()),
        _ => None,
    });
    assert!(
        matches!(&first, Some(PermissionChoice::Selected { option_id, .. }) if option_id == "allow-once"),
        "{first:?}"
    );
    host.shutdown();
}

/// The tools of the permission requests the lawyer saw, by the agent's tool call id (`None` for
/// a request whose tool call the host never logged).
fn requested_tools(events: &Events) -> Vec<Option<String>> {
    let started: std::collections::BTreeMap<_, String> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ToolCallStarted(started) => {
                Some((started.tool_call, started.acp_id.to_string()))
            }
            _ => None,
        })
        .collect();
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionRequested(requested) => Some(
                requested
                    .tool_call
                    .and_then(|tool| started.get(&tool).cloned()),
            ),
            _ => None,
        })
        .collect()
}

/// Codex's approvals of the guarded browser server: a click and an eval (correlated with their
/// tool calls) and a snapshot and an eval (standalone) are answered `allow_session` by the host
/// with nothing logged, the guard refusing eval itself; a case-law search and a shell command
/// reach the lawyer. The scenario checks each answer the agent gets.
#[test]
fn codex_browser_tools_are_allowed_unseen() {
    const CODEX: &str = "codex-acp";
    let dir = Dir::new(
        "codex_browser_approvals",
        &[(CODEX, Scenario::Local("codex-browser-approvals"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &["allow_once", "approved"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let seen = requested_tools(&events);
    assert_eq!(seen.len(), 2, "{}", events.dump());
    assert_eq!(seen[0].as_deref(), Some("tc-law"));
    assert_eq!(seen[1].as_deref(), Some("tc-shell"));
    assert!(
        !seen
            .iter()
            .any(|tool| matches!(tool.as_deref(), Some("tc-click" | "tc-eval"))),
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A Claude session's requests are never answered by the host, even in Codex's browser shapes
/// with `allow_session` offered.
#[test]
fn a_claude_sessions_browser_approval_reaches_the_lawyer() {
    const CLAUDE: &str = "claude-acp";
    let dir = Dir::new(
        "claude_browser_approval",
        &[(CLAUDE, Scenario::Local("claude-browser-approval"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Find the judgment.");
    answer_from(&host, &mut events, thread, 0, &["allow_once", "allow_once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(requested_tools(&events).len(), 2, "{}", events.dump());
    host.shutdown();
}
