//! Permission requests: answers, refusals while a turn runs, the lawyer-edits flag.

use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand,
    MatterPath, PermissionChoice, StopReason, StreamId, ThreadId,
};

use crate::observe::Events;
use crate::support::{Dir, Running, Scenario, matter_with_thread, send_prompt};
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
fn prompt_refused_while_turn_runs() {
    let dir = kinds_dir("prompt_refused_while_turn_runs");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], PERM);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);

    let id = crate::support::command_id();
    let error = host
        .dispatch_as(id, send_prompt(thread, "Another question."))
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::ThreadTurnRunning, "{error}");
    assert!(error.retryable());

    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(
        ended.stop,
        StopReason::EndTurn,
        "the agent saw a second prompt"
    );
    let user_messages = events.count(|event| matches!(event, DomainEvent::UserMessage(_)));
    assert_eq!(
        user_messages,
        1,
        "the refused prompt was logged: {}",
        events.dump()
    );

    // No receipt was stored: the same command id now runs as a new prompt. The scenario has no
    // second prompt, so the agent ends that turn with a mismatch, which is not checked here.
    let outcome = host
        .dispatch_as(id, send_prompt(thread, "Another question."))
        .unwrap_or_else(|e| panic!("the refused command left a receipt: {e}"));
    let CommandOutcome::PromptAccepted { turn: second, .. } = outcome else {
        panic!("SendPrompt answered {outcome:?}");
    };
    assert_ne!(second, turn);
    events.until_turn_ended(second);
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
